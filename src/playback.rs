use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, StreamConfig};
use ringbuf::{HeapRb, traits::*};

use crate::decode::DecodedAudio;

const RINGBUF_CAPACITY: usize = 44100 * 2 * 2;

#[derive(Debug)]
pub enum PlaybackCommand {
    Load(DecodedAudio),
    Play,
    Pause,
    SeekFrame(u64),
    SetVolume(f32),
    Shutdown,
}

#[derive(Debug)]
pub struct PlaybackStatus {
    pub playhead_frame: AtomicU64,
    pub duration_frames: AtomicU64,
    pub sample_rate: AtomicU64,
    pub playing: AtomicBool,
    pub seek_generation: AtomicU64,
}

impl PlaybackStatus {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            playhead_frame: AtomicU64::new(0),
            duration_frames: AtomicU64::new(0),
            sample_rate: AtomicU64::new(44_100),
            playing: AtomicBool::new(false),
            seek_generation: AtomicU64::new(0),
        })
    }

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

struct FeederState {
    samples: Arc<Vec<f32>>,
    channels: usize,
    playhead_sample: usize,
    volume: f32,
    playing: bool,
}

pub struct PlaybackEngine {
    cmd_tx: mpsc::Sender<PlaybackCommand>,
    status: Arc<PlaybackStatus>,
    output_sample_rate: u32,
    _feeder: JoinHandle<()>,
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
        let output_sample_rate = stream_config.sample_rate;

        let rb = HeapRb::<f32>::new(RINGBUF_CAPACITY);
        let (producer, mut consumer) = rb.split();

        let status = PlaybackStatus::new();
        let status_for_callback = Arc::clone(&status);

        let (cmd_tx, cmd_rx) = mpsc::channel::<PlaybackCommand>();

        let err_fn = |err| eprintln!("audio stream error: {err}");
        let mut last_seek = 0u64;
        let mut drain_buf = [0.0f32; 1024];

        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                stream_config.clone(),
                move |data: &mut [f32], _| {
                    drain_if_seeked(&mut consumer, &status_for_callback, &mut last_seek, &mut drain_buf);
                    let popped = consumer.pop_slice(data);
                    for sample in &mut data[popped..] {
                        *sample = 0.0;
                    }
                    maybe_stop_at_end(&status_for_callback, popped);
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_output_stream(
                stream_config.clone(),
                move |data: &mut [i16], _| {
                    drain_if_seeked(&mut consumer, &status_for_callback, &mut last_seek, &mut drain_buf);
                    let mut temp = vec![0.0f32; data.len()];
                    let popped = consumer.pop_slice(&mut temp);
                    for (out, sample) in data.iter_mut().zip(temp.iter()) {
                        *out = Sample::from_sample(*sample);
                    }
                    for out in data.iter_mut().skip(popped) {
                        *out = Sample::EQUILIBRIUM;
                    }
                    maybe_stop_at_end(&status_for_callback, popped);
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_output_stream(
                stream_config.clone(),
                move |data: &mut [u16], _| {
                    drain_if_seeked(&mut consumer, &status_for_callback, &mut last_seek, &mut drain_buf);
                    let mut temp = vec![0.0f32; data.len()];
                    let popped = consumer.pop_slice(&mut temp);
                    for (out, sample) in data.iter_mut().zip(temp.iter()) {
                        *out = Sample::from_sample(*sample);
                    }
                    for out in data.iter_mut().skip(popped) {
                        *out = Sample::EQUILIBRIUM;
                    }
                    maybe_stop_at_end(&status_for_callback, popped);
                },
                err_fn,
                None,
            )?,
            other => anyhow::bail!("unsupported sample format: {other:?}"),
        };

        stream.play()?;

        let status_for_feeder = Arc::clone(&status);
        let feeder = thread::spawn(move || {
            feeder_loop(cmd_rx, producer, status_for_feeder, output_sample_rate);
        });

        Ok(Self {
            cmd_tx,
            status,
            output_sample_rate,
            _feeder: feeder,
            _stream: stream,
        })
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
        let rate = self.status.sample_rate.load(Ordering::Relaxed).max(1);
        let frame = (secs.max(0.0) * rate as f64).round() as u64;
        self.cmd_tx.send(PlaybackCommand::SeekFrame(frame))?;
        Ok(())
    }

    pub fn set_volume(&self, volume: f32) -> anyhow::Result<()> {
        self.cmd_tx
            .send(PlaybackCommand::SetVolume(volume.clamp(0.0, 1.0)))?;
        Ok(())
    }

    pub fn output_sample_rate(&self) -> u32 {
        self.output_sample_rate
    }
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(PlaybackCommand::Shutdown);
    }
}

fn drain_if_seeked(
    consumer: &mut ringbuf::HeapCons<f32>,
    status: &PlaybackStatus,
    last_seek: &mut u64,
    scratch: &mut [f32],
) {
    let generation = status.seek_generation.load(Ordering::Acquire);
    if generation != *last_seek {
        while consumer.pop_slice(scratch) > 0 {}
        *last_seek = generation;
    }
}

fn maybe_stop_at_end(status: &PlaybackStatus, popped: usize) {
    if popped == 0 && status.playing.load(Ordering::Relaxed) {
        let frame = status.playhead_frame.load(Ordering::Relaxed);
        let duration = status.duration_frames.load(Ordering::Relaxed);
        if duration > 0 && frame >= duration {
            status.playing.store(false, Ordering::Relaxed);
        }
    }
}

fn feeder_loop(
    cmd_rx: mpsc::Receiver<PlaybackCommand>,
    mut producer: ringbuf::HeapProd<f32>,
    status: Arc<PlaybackStatus>,
    output_sample_rate: u32,
) {
    let mut state = FeederState {
        samples: Arc::new(Vec::new()),
        channels: 2,
        playhead_sample: 0,
        volume: 0.85,
        playing: false,
    };

    loop {
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                PlaybackCommand::Load(mut audio) => {
                    if audio.sample_rate != output_sample_rate {
                        audio.samples = resample_interleaved(
                            &audio.samples,
                            audio.channels,
                            audio.sample_rate,
                            output_sample_rate,
                        );
                        audio.sample_rate = output_sample_rate;
                    }

                    let frames = audio.samples.len() / audio.channels.max(1);
                    status.duration_frames.store(frames as u64, Ordering::Relaxed);
                    status
                        .sample_rate
                        .store(audio.sample_rate as u64, Ordering::Relaxed);
                    status.playhead_frame.store(0, Ordering::Relaxed);
                    status.playing.store(false, Ordering::Relaxed);
                    status.seek_generation.fetch_add(1, Ordering::Release);

                    state.channels = audio.channels.max(1);
                    state.playhead_sample = 0;
                    state.samples = Arc::new(audio.samples);
                    state.playing = false;
                }
                PlaybackCommand::Play => {
                    state.playing = true;
                    status.playing.store(true, Ordering::Relaxed);
                }
                PlaybackCommand::Pause => {
                    state.playing = false;
                    status.playing.store(false, Ordering::Relaxed);
                }
                PlaybackCommand::SeekFrame(frame) => {
                    let sample = (frame as usize).saturating_mul(state.channels);
                    state.playhead_sample = sample.min(state.samples.len());
                    status.playhead_frame.store(frame, Ordering::Relaxed);
                    status.seek_generation.fetch_add(1, Ordering::Release);
                }
                PlaybackCommand::SetVolume(volume) => {
                    state.volume = volume;
                }
                PlaybackCommand::Shutdown => return,
            }
        }

        if state.playing && state.playhead_sample < state.samples.len() {
            let chunk_size = 2048.min(state.samples.len() - state.playhead_sample);
            let mut chunk = vec![0.0f32; chunk_size];
            for (out, &sample) in chunk
                .iter_mut()
                .zip(&state.samples[state.playhead_sample..state.playhead_sample + chunk_size])
            {
                *out = sample * state.volume;
            }

            let pushed = producer.push_slice(&chunk);
            state.playhead_sample += pushed;
            let frame = (state.playhead_sample / state.channels) as u64;
            status.playhead_frame.store(frame, Ordering::Relaxed);

            let duration = status.duration_frames.load(Ordering::Relaxed);
            if duration > 0 && frame >= duration {
                state.playing = false;
                status.playing.store(false, Ordering::Relaxed);
            }
        } else {
            thread::sleep(std::time::Duration::from_millis(4));
        }
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
