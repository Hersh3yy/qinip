use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, StreamConfig};

use crate::decode::DecodedAudio;
use crate::dev;
use crate::eq::{EqGains, LiveEq};

#[derive(Debug)]
enum PlaybackCommand {
    Load(Arc<DecodedAudio>),
    Play,
    Pause,
    SeekFrame(u64),
    SetVolume(f32),
    SetEq(EqGains),
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
    duration_frames: u64,
}

struct SharedPlayback {
    track: Mutex<Option<TrackBuffer>>,
    /// One LiveEq per output channel.
    eq: Mutex<Vec<LiveEq>>,
    playhead_frame: AtomicU64,
    duration_frames: AtomicU64,
    sample_rate: AtomicU64,
    playing: AtomicBool,
    volume: AtomicU32,
    eq_low: AtomicU32,
    eq_low_mid: AtomicU32,
    eq_hi_mid: AtomicU32,
    eq_hi: AtomicU32,
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
        let output_channels = stream_config.channels as usize;
        let output_sample_rate = stream_config.sample_rate;

        dev::log(format!(
            "audio output: {output_sample_rate} Hz | {output_channels} channels"
        ));

        let eq_channels: Vec<LiveEq> = (0..output_channels.max(1))
            .map(|_| LiveEq::new(output_sample_rate))
            .collect();

        let unity = EqGains::default();
        let shared = Arc::new(SharedPlayback {
            track: Mutex::new(None),
            eq: Mutex::new(eq_channels),
            playhead_frame: AtomicU64::new(0),
            duration_frames: AtomicU64::new(0),
            sample_rate: AtomicU64::new(output_sample_rate as u64),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(0.85f32.to_bits()),
            eq_low: AtomicU32::new(unity.low.to_bits()),
            eq_low_mid: AtomicU32::new(unity.low_mid.to_bits()),
            eq_hi_mid: AtomicU32::new(unity.hi_mid.to_bits()),
            eq_hi: AtomicU32::new(unity.hi.to_bits()),
            output_channels,
            output_sample_rate,
        });

        let status = Arc::new(PlaybackStatus {
            playhead_frame: AtomicU64::new(0),
            duration_frames: AtomicU64::new(0),
            sample_rate: AtomicU64::new(output_sample_rate as u64),
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
            // I16/U16 devices: cpal wants ints, but `render_output_f32` only
            // knows how to fill `f32`. `scratch` is created once here, moved
            // into the closure, and reused (resized, not reallocated) on
            // every callback — the real-time audio thread must never
            // allocate, and `vec![...]` inside the closure body would.
            SampleFormat::I16 => {
                let mut scratch: Vec<f32> = Vec::new();
                device.build_output_stream(
                    stream_config.clone(),
                    move |data: &mut [i16], _| {
                        fill_from_f32_scratch(data, &mut scratch, &shared_cb, &status_cb);
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                let mut scratch: Vec<f32> = Vec::new();
                device.build_output_stream(
                    stream_config,
                    move |data: &mut [u16], _| {
                        fill_from_f32_scratch(data, &mut scratch, &shared_cb, &status_cb);
                    },
                    err_fn,
                    None,
                )?
            }
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

    /// Queue a track for playback. `audio` is shared (`Arc`), not copied here —
    /// the caller's `audio_cache` entry and this command both point at the
    /// same decoded buffer. See `resolve_track_samples` for the one place a
    /// real copy can still happen (resampling to the output device's rate).
    pub fn load(&self, audio: Arc<DecodedAudio>) -> anyhow::Result<()> {
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

    pub fn set_eq(&self, gains: EqGains) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::SetEq(gains.clamped()))?;
        Ok(())
    }

    fn apply(&self, cmd: PlaybackCommand) {
        match cmd {
            PlaybackCommand::Load(audio) => {
                let start = std::time::Instant::now();

                let channels = audio.channels.max(1);
                let output_rate = self.shared.output_sample_rate;
                let samples = self.resolve_track_samples(&audio, channels, output_rate);
                let duration_frames = (samples.len() / channels) as u64;

                let track = TrackBuffer {
                    samples,
                    channels,
                    duration_frames,
                };

                self.shared
                    .duration_frames
                    .store(duration_frames, Ordering::Relaxed);
                self.shared
                    .sample_rate
                    .store(output_rate as u64, Ordering::Relaxed);
                self.shared.playhead_frame.store(0, Ordering::Relaxed);
                self.shared.playing.store(false, Ordering::Relaxed);
                self.reset_eq();

                *self.shared.track.lock().expect("playback track lock") = Some(track);

                dev::log(format!(
                    "loaded track: {duration_frames} frames, {channels} ch @ {output_rate} Hz"
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
                self.reset_eq();
                dev::log(format!("seek -> frame {frame}"));
            }
            PlaybackCommand::SetVolume(volume) => {
                self.shared
                    .volume
                    .store(volume.to_bits(), Ordering::Relaxed);
            }
            PlaybackCommand::SetEq(gains) => {
                self.shared.eq_low.store(gains.low.to_bits(), Ordering::Relaxed);
                self.shared
                    .eq_low_mid
                    .store(gains.low_mid.to_bits(), Ordering::Relaxed);
                self.shared
                    .eq_hi_mid
                    .store(gains.hi_mid.to_bits(), Ordering::Relaxed);
                self.shared.eq_hi.store(gains.hi.to_bits(), Ordering::Relaxed);
                dev::log(format!(
                    "eq L={:.2} LM={:.2} HM={:.2} H={:.2}",
                    gains.low, gains.low_mid, gains.hi_mid, gains.hi
                ));
            }
        }
    }

    /// Get samples at the output device's rate, sharing the decoded buffer
    /// whenever possible instead of copying it.
    ///
    /// - Same rate as the output device (the common case): just bumps the
    ///   `Arc`'s reference count — zero bytes copied, no matter how many
    ///   times this track gets played, replayed, or autoplayed to next.
    /// - Different rate: a real copy is unavoidable, since resampling
    ///   produces new sample values, not just a reference to old ones.
    fn resolve_track_samples(
        &self,
        audio: &DecodedAudio,
        channels: usize,
        output_rate: u32,
    ) -> Arc<[f32]> {
        if audio.sample_rate == output_rate {
            return Arc::clone(&audio.samples);
        }

        dev::log(format!("resampling {} -> {output_rate} Hz", audio.sample_rate));
        let resampled = resample_interleaved(&audio.samples, channels, audio.sample_rate, output_rate);
        Arc::from(resampled)
    }

    fn reset_eq(&self) {
        if let Ok(mut eqs) = self.shared.eq.lock() {
            for eq in eqs.iter_mut() {
                eq.reset(self.shared.output_sample_rate);
            }
        }
    }
}

fn read_gains(shared: &SharedPlayback) -> EqGains {
    EqGains {
        low: f32::from_bits(shared.eq_low.load(Ordering::Relaxed)),
        low_mid: f32::from_bits(shared.eq_low_mid.load(Ordering::Relaxed)),
        hi_mid: f32::from_bits(shared.eq_hi_mid.load(Ordering::Relaxed)),
        hi: f32::from_bits(shared.eq_hi.load(Ordering::Relaxed)),
    }
}

/// Renders into a reused `f32` scratch buffer, then converts sample-by-sample
/// into the device's native integer format. `scratch` is caller-owned and
/// reused across calls so this never allocates on the audio callback thread.
fn fill_from_f32_scratch<S: Sample + cpal::FromSample<f32>>(
    data: &mut [S],
    scratch: &mut Vec<f32>,
    shared: &SharedPlayback,
    status: &PlaybackStatus,
) {
    scratch.clear();
    scratch.resize(data.len(), 0.0);
    render_output_f32(scratch, shared, status);
    for (out, sample) in data.iter_mut().zip(scratch.iter()) {
        *out = Sample::from_sample(*sample);
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

    let Ok(mut eqs) = shared.eq.try_lock() else {
        return;
    };

    let src_channels = src_channels.max(1);
    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
    let gains = read_gains(shared);
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
            let dry = samples.get(idx).copied().unwrap_or(0.0);
            let eq_ch = out_ch.min(eqs.len().saturating_sub(1));
            let wet = eqs[eq_ch].process(dry, gains);
            data[out_frame * out_channels + out_ch] = wet * volume;
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
