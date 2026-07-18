use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, StreamConfig};

use crate::decode::DecodedAudio;
use crate::dev;

#[derive(Debug)]
enum PlaybackCommand {
    Load(DecodedAudio),
    Play,
    Pause,
    SeekFrame(u64),
    SetVolume(f32),
}

#[derive(Debug)]
pub struct PlaybackStatus {
    playhead_frame: AtomicU64,
    duration_frames: AtomicU64,
    sample_rate: AtomicU64,
    playing: AtomicBool,
}

impl PlaybackStatus {
    pub fn position_secs(&self) -> f64 {
        let rate = self.sample_rate.load(Ordering::Relaxed).max(1);
        self.playhead_frame.load(Ordering::Relaxed) as f64 / rate as f64
    }

    pub fn duration_secs(&self) -> f64 {
        let rate = self.sample_rate.load(Ordering::Relaxed).max(1);
        self.duration_frames.load(Ordering::Relaxed) as f64 / rate as f64
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
}

struct TrackBuffer {
    samples: Arc<[f32]>,
    channels: usize,
    sample_rate: u32,
    duration_frames: u64,
}

struct SharedPlayback {
    track: Mutex<Option<TrackBuffer>>,
    playhead_frame: AtomicU64,
    duration_frames: AtomicU64,
    sample_rate: AtomicU64,
    playing: AtomicBool,
    volume: AtomicU32,
    output_channels: usize,
    output_sample_rate: u32,
}

pub struct PlaybackEngine {
    shared: Arc<SharedPlayback>,
    cmd_rx: mpsc::Receiver<PlaybackCommand>,
    cmd_tx: mpsc::Sender<PlaybackCommand>,
    status: Arc<PlaybackStatus>,
    _stream: cpal::Stream,
}

impl PlaybackEngine {
    pub fn new() -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("no output audio device"))?;

        let default_config = device.default_output_config()?;
        let sample_format = default_config.sample_format();
        let stream_config: StreamConfig = default_config.config();

        dev::log(format!(
            "audio output: {} Hz | {} channels",
            stream_config.sample_rate,
            stream_config.channels
        ));

        let shared = Arc::new(SharedPlayback {
            track: Mutex::new(None),
            playhead_frame: AtomicU64::new(0),
            duration_frames: AtomicU64::new(0),
            sample_rate: AtomicU64::new(stream_config.sample_rate as u64),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(0.85f32.to_bits()),
            output_channels: stream_config.channels as usize,
            output_sample_rate: stream_config.sample_rate,
        });

        let status = Arc::new(PlaybackStatus {
            playhead_frame: AtomicU64::new(0),
            duration_frames: AtomicU64::new(0),
            sample_rate: AtomicU64::new(stream_config.sample_rate as u64),
            playing: AtomicBool::new(false),
        });

        let shared_cb = Arc::clone(&shared);
        let status_cb = Arc::clone(&status);

        let err_fn = |err| eprintln!("audio stream error: {err}");
        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                stream_config.clone(),
                move |data: &mut [f32], _| {
                    render_output_f32(data, &shared_cb, &status_cb);
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_output_stream(
                stream_config.clone(),
                move |data: &mut [i16], _| {
                    let mut temp = vec![0.0f32; data.len()];
                    render_output_f32(&mut temp, &shared_cb, &status_cb);
                    for (out, sample) in data.iter_mut().zip(temp) {
                        *out = Sample::from_sample(sample);
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_output_stream(
                stream_config,
                move |data: &mut [u16], _| {
                    let mut temp = vec![0.0f32; data.len()];
                    render_output_f32(&mut temp, &shared_cb, &status_cb);
                    for (out, sample) in data.iter_mut().zip(temp) {
                        *out = Sample::from_sample(sample);
                    }
                },
                err_fn,
                None,
            )?,
            other => anyhow::bail!("unsupported sample format: {other:?}"),
        };

        stream.play()?;
        dev::log("cpal output stream started");

        let (cmd_tx, cmd_rx) = mpsc::channel();

        Ok(Self {
            shared,
            cmd_rx,
            cmd_tx,
            status,
            _stream: stream,
        })
    }

    pub fn poll(&self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            self.apply(cmd);
        }

        self.status.playhead_frame.store(
            self.shared.playhead_frame.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.status.duration_frames.store(
            self.shared.duration_frames.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.status.sample_rate.store(
            self.shared.sample_rate.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.status.playing.store(
            self.shared.playing.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
    }

    pub fn status(&self) -> Arc<PlaybackStatus> {
        Arc::clone(&self.status)
    }

    pub fn load(&self, audio: DecodedAudio) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::Load(audio))?;
        Ok(())
    }

    pub fn play(&self) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::Play)?;
        Ok(())
    }

    pub fn pause(&self) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::Pause)?;
        Ok(())
    }

    pub fn seek_secs(&self, secs: f64) -> anyhow::Result<()> {
        let rate = self.shared.sample_rate.load(Ordering::Relaxed).max(1);
        let frame = (secs.max(0.0) * rate as f64).round() as u64;
        self.cmd_tx.send(PlaybackCommand::SeekFrame(frame))?;
        Ok(())
    }

    pub fn set_volume(&self, volume: f32) -> anyhow::Result<()> {
        self.cmd_tx
            .send(PlaybackCommand::SetVolume(volume.clamp(0.0, 1.0)))?;
        Ok(())
    }

    fn apply(&self, cmd: PlaybackCommand) {
        match cmd {
            PlaybackCommand::Load(audio) => {
                let start = std::time::Instant::now();
                let mut audio = audio;
                if audio.sample_rate != self.shared.output_sample_rate {
                    dev::log(format!(
                        "resampling {} -> {} Hz",
                        audio.sample_rate, self.shared.output_sample_rate
                    ));
                    audio.samples = resample_interleaved(
                        &audio.samples,
                        audio.channels,
                        audio.sample_rate,
                        self.shared.output_sample_rate,
                    );
                    audio.sample_rate = self.shared.output_sample_rate;
                }

                let channels = audio.channels.max(1);
                let duration_frames = (audio.samples.len() / channels) as u64;

                let track = TrackBuffer {
                    samples: Arc::from(audio.samples.into_boxed_slice()),
                    channels,
                    sample_rate: audio.sample_rate,
                    duration_frames,
                };

                self.shared
                    .duration_frames
                    .store(duration_frames, Ordering::Relaxed);
                self.shared
                    .sample_rate
                    .store(audio.sample_rate as u64, Ordering::Relaxed);
                self.shared.playhead_frame.store(0, Ordering::Relaxed);
                self.shared.playing.store(false, Ordering::Relaxed);

                *self.shared.track.lock().expect("playback track lock") = Some(track);

                dev::log(format!(
                    "loaded track: {duration_frames} frames, {channels} ch @ {} Hz",
                    audio.sample_rate
                ));
                dev::log_timing("playback load", start);
            }
            PlaybackCommand::Play => {
                self.shared.playing.store(true, Ordering::Relaxed);
                dev::log("play");
            }
            PlaybackCommand::Pause => {
                self.shared.playing.store(false, Ordering::Relaxed);
                dev::log("pause");
            }
            PlaybackCommand::SeekFrame(frame) => {
                let duration = self.shared.duration_frames.load(Ordering::Relaxed);
                let frame = frame.min(duration);
                self.shared.playhead_frame.store(frame, Ordering::Relaxed);
                dev::log(format!("seek -> frame {frame}"));
            }
            PlaybackCommand::SetVolume(volume) => {
                self.shared
                    .volume
                    .store(volume.to_bits(), Ordering::Relaxed);
            }
        }
    }
}

fn render_output_f32(data: &mut [f32], shared: &SharedPlayback, status: &PlaybackStatus) {
    let out_channels = shared.output_channels.max(1);
    let out_frames = data.len() / out_channels;

    for sample in data.iter_mut() {
        *sample = 0.0;
    }

    let playing = shared.playing.load(Ordering::Relaxed);
    if !playing || out_frames == 0 {
        return;
    }

    let track = match shared.track.try_lock() {
        Ok(guard) => guard.as_ref().map(|t| {
            (
                Arc::clone(&t.samples),
                t.channels,
                t.duration_frames,
            )
        }),
        Err(_) => return,
    };

    let Some((samples, src_channels, duration_frames)) = track else {
        return;
    };

    let src_channels = src_channels.max(1);
    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
    let mut frame = shared.playhead_frame.load(Ordering::Relaxed);
    let mut rendered_frames = 0u64;

    for out_frame in 0..out_frames {
        if frame >= duration_frames {
            shared.playing.store(false, Ordering::Relaxed);
            break;
        }

        let base = frame as usize * src_channels;
        for out_ch in 0..out_channels {
            let src_ch = out_ch % src_channels;
            let idx = base + src_ch;
            let sample = samples.get(idx).copied().unwrap_or(0.0) * volume;
            data[out_frame * out_channels + out_ch] = sample;
        }

        frame += 1;
        rendered_frames += 1;
    }

    if rendered_frames > 0 {
        shared.playhead_frame.store(frame, Ordering::Relaxed);
        status.playhead_frame.store(frame, Ordering::Relaxed);
    }

    if frame >= duration_frames && duration_frames > 0 {
        shared.playing.store(false, Ordering::Relaxed);
        status.playing.store(false, Ordering::Relaxed);
    }
}

fn resample_interleaved(
    samples: &[f32],
    channels: usize,
    from_rate: u32,
    to_rate: u32,
) -> Vec<f32> {
    if from_rate == to_rate || channels == 0 {
        return samples.to_vec();
    }

    let in_frames = samples.len() / channels;
    let out_frames =
        ((in_frames as f64 * to_rate as f64 / from_rate as f64).round() as usize).max(1);
    let mut out = vec![0.0f32; out_frames * channels];

    for out_frame in 0..out_frames {
        let src_pos = out_frame as f64 * from_rate as f64 / to_rate as f64;
        let src_frame = src_pos.floor() as usize;
        let frac = (src_pos - src_frame as f64) as f32;

        for ch in 0..channels {
            let a = samples
                .get(src_frame * channels + ch)
                .copied()
                .unwrap_or(0.0);
            let b = samples
                .get((src_frame + 1) * channels + ch)
                .copied()
                .unwrap_or(a);
            out[out_frame * channels + ch] = a + (b - a) * frac;
        }
    }

    out
}
