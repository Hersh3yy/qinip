use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, StreamConfig};

use crate::decode::DecodedAudio;
use crate::dev;
use crate::eq::{EqGains, LiveEq};

#[derive(Debug)]
enum PlaybackCommand {
    Load(Arc<DecodedAudio>),
    QueueNext(Arc<DecodedAudio>),
    ClearNext,
    /// Forget the loaded track entirely (Clear Playlist).
    Unload,
    Play,
    Pause,
    SeekFrame(u64),
    SetVolume(f32),
    SetEq(EqGains),
    SetLoop { start_frame: u64, end_frame: u64 },
    ClearLoop,
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
    /// Preloaded next track for gapless autoplay — swapped in by the audio callback.
    next_track: Mutex<Option<TrackBuffer>>,
    auto_advanced: AtomicBool,
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
    loop_active: AtomicBool,
    loop_start_frame: AtomicU64,
    loop_end_frame: AtomicU64,
    /// Latest news from the stream's own threads, as a `StreamEvent` code; `DeviceLost`
    /// outranks `DeviceChanged` until the UI takes it.
    stream_event: AtomicU8,
}

/// What the output stream reported since the UI last asked (`take_stream_event`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamEvent {
    None,
    /// The system default output changed and CoreAudio rerouted the running stream to it.
    /// Nothing to do; the sound simply follows the new device.
    DeviceChanged,
    /// The device vanished (unplugged, Bluetooth dropped) and no default replaced it. cpal
    /// paused the stream; it never resumes. Call `drop_output()` and open a new engine
    /// once a device is back.
    DeviceLost,
}

impl StreamEvent {
    const CHANGED: u8 = 1;
    const LOST: u8 = 2;

    fn from_code(code: u8) -> Self {
        match code {
            Self::CHANGED => Self::DeviceChanged,
            Self::LOST => Self::DeviceLost,
            _ => Self::None,
        }
    }
}

/// The stream's error callback: device news becomes a `StreamEvent` for the UI thread;
/// anything else is printed, as before.
fn stream_error_sink(shared: Arc<SharedPlayback>) -> impl FnMut(cpal::Error) + Send + 'static {
    move |err: cpal::Error| {
        let code = match err.kind() {
            cpal::ErrorKind::DeviceChanged => StreamEvent::CHANGED,
            cpal::ErrorKind::DeviceNotAvailable => StreamEvent::LOST,
            _ => {
                eprintln!("audio stream error: {err}");
                return;
            }
        };
        shared.stream_event.fetch_max(code, Ordering::Relaxed);
        dev::log(format!("audio stream: {err}"));
    }
}

pub struct PlaybackEngine {
    shared: Arc<SharedPlayback>,
    cmd_rx: mpsc::Receiver<PlaybackCommand>,
    cmd_tx: mpsc::Sender<PlaybackCommand>,
    status: Arc<PlaybackStatus>,
    /// `None` in silent mode (no output device at startup). The engine still
    /// accepts every command so the UI keeps working; nothing is heard.
    stream: Option<cpal::Stream>,
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

        let (shared, status) = Self::shared_state(output_channels, output_sample_rate);

        let shared_cb = Arc::clone(&shared);
        let status_cb = Arc::clone(&status);

        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                stream_config,
                move |data: &mut [f32], _| {
                    render_output_f32(data, &shared_cb, &status_cb);
                },
                stream_error_sink(Arc::clone(&shared)),
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
                    stream_config,
                    move |data: &mut [i16], _| {
                        fill_from_f32_scratch(data, &mut scratch, &shared_cb, &status_cb);
                    },
                    stream_error_sink(Arc::clone(&shared)),
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
                    stream_error_sink(Arc::clone(&shared)),
                    None,
                )?
            }
            other => anyhow::bail!("unsupported sample format: {other:?}"),
        };

        stream.play()?;
        dev::log("cpal output stream started");

        Ok(Self::from_parts(shared, status, Some(stream)))
    }

    /// An engine with no output stream, used when `new()` fails (no device,
    /// unsupported format). Commands are accepted and state is tracked, but
    /// `play()` is refused so the UI never shows a "playing" track that is silent.
    pub fn silent() -> Self {
        let (shared, status) = Self::shared_state(2, 44_100);
        dev::log("audio output unavailable, running silent");
        Self::from_parts(shared, status, None)
    }

    pub fn has_output(&self) -> bool {
        self.stream.is_some()
    }

    /// Device news since the last call, then cleared.
    pub fn take_stream_event(&self) -> StreamEvent {
        StreamEvent::from_code(self.shared.stream_event.swap(0, Ordering::Relaxed))
    }

    /// Forget a stream the host reported dead (`StreamEvent::DeviceLost`): `has_output()`
    /// is false until a new engine is built, and nothing counts as playing meanwhile.
    pub fn drop_output(&mut self) {
        self.stream = None;
        self.shared.playing.store(false, Ordering::Relaxed);
        self.status.playing.store(false, Ordering::Relaxed);
    }

    fn from_parts(
        shared: Arc<SharedPlayback>,
        status: Arc<PlaybackStatus>,
        stream: Option<cpal::Stream>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        Self {
            shared,
            cmd_rx,
            cmd_tx,
            status,
            stream,
        }
    }

    fn shared_state(
        output_channels: usize,
        output_sample_rate: u32,
    ) -> (Arc<SharedPlayback>, Arc<PlaybackStatus>) {
        let eq_channels: Vec<LiveEq> = (0..output_channels.max(1))
            .map(|_| LiveEq::new(output_sample_rate))
            .collect();

        let unity = EqGains::default();
        let shared = Arc::new(SharedPlayback {
            track: Mutex::new(None),
            next_track: Mutex::new(None),
            auto_advanced: AtomicBool::new(false),
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
            loop_active: AtomicBool::new(false),
            loop_start_frame: AtomicU64::new(0),
            loop_end_frame: AtomicU64::new(0),
            stream_event: AtomicU8::new(0),
        });

        let status = Arc::new(PlaybackStatus {
            playhead_frame: AtomicU64::new(0),
            duration_frames: AtomicU64::new(0),
            sample_rate: AtomicU64::new(output_sample_rate as u64),
            playing: AtomicBool::new(false),
        });

        (shared, status)
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
        self.cmd_tx.send(PlaybackCommand::ClearNext)?;
        self.cmd_tx.send(PlaybackCommand::Load(audio))?;
        Ok(())
    }

    /// Stage the next track so the audio callback can swap to it with no gap.
    pub fn queue_next(&self, audio: Arc<DecodedAudio>) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::QueueNext(audio))?;
        Ok(())
    }

    /// Stop and forget the loaded track; the transport reads 0:00 / 0:00.
    pub fn unload(&self) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::Unload)?;
        Ok(())
    }

    /// Drop a staged gapless track (e.g. shuffle/repeat changed after preload).
    pub fn clear_queued(&self) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::ClearNext)?;
        Ok(())
    }

    /// `true` once after the audio thread auto-swapped to a queued track.
    pub fn take_auto_advanced(&self) -> bool {
        self.shared
            .auto_advanced
            .swap(false, Ordering::Relaxed)
    }

    pub fn play(&self) -> anyhow::Result<()> {
        if !self.has_output() {
            anyhow::bail!("no audio output device");
        }
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

    pub fn set_loop_secs(&self, start_secs: f64, end_secs: f64) -> anyhow::Result<()> {
        let rate = self.shared.sample_rate.load(Ordering::Relaxed).max(1);
        let start_frame = (start_secs.max(0.0) * rate as f64).round() as u64;
        let end_frame = (end_secs.max(0.0) * rate as f64).round() as u64;
        self.cmd_tx.send(PlaybackCommand::SetLoop {
            start_frame,
            end_frame,
        })?;
        Ok(())
    }

    pub fn clear_loop(&self) -> anyhow::Result<()> {
        self.cmd_tx.send(PlaybackCommand::ClearLoop)?;
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
                self.shared.auto_advanced.store(false, Ordering::Relaxed);
                self.clear_loop_internal();
                self.clear_queued_next();
                self.reset_eq();

                *self.shared.track.lock().expect("playback track lock") = Some(track);

                dev::log(format!(
                    "loaded track: {duration_frames} frames, {channels} ch @ {output_rate} Hz"
                ));
                dev::log_timing("playback load", start);
            }
            PlaybackCommand::QueueNext(audio) => {
                let channels = audio.channels.max(1);
                let output_rate = self.shared.output_sample_rate;
                let samples = self.resolve_track_samples(&audio, channels, output_rate);
                let duration_frames = (samples.len() / channels) as u64;
                let next = TrackBuffer {
                    samples,
                    channels,
                    duration_frames,
                };
                *self.shared.next_track.lock().expect("next track lock") = Some(next);
                dev::log(format!("queued next: {duration_frames} frames"));
            }
            PlaybackCommand::ClearNext => {
                self.clear_queued_next();
            }
            PlaybackCommand::Unload => {
                self.shared.playing.store(false, Ordering::Relaxed);
                self.clear_queued_next();
                self.clear_loop_internal();
                *self.shared.track.lock().expect("playback track lock") = None;
                self.shared.playhead_frame.store(0, Ordering::Relaxed);
                self.shared.duration_frames.store(0, Ordering::Relaxed);
                dev::log("unload");
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
            PlaybackCommand::SetLoop {
                start_frame,
                end_frame,
            } => {
                if end_frame <= start_frame {
                    self.clear_loop_internal();
                    return;
                }
                self.shared
                    .loop_start_frame
                    .store(start_frame, Ordering::Relaxed);
                self.shared
                    .loop_end_frame
                    .store(end_frame, Ordering::Relaxed);
                self.shared.loop_active.store(true, Ordering::Relaxed);
                let current = self.shared.playhead_frame.load(Ordering::Relaxed);
                if current >= end_frame {
                    self.shared
                        .playhead_frame
                        .store(start_frame, Ordering::Relaxed);
                }
                dev::log(format!(
                    "loop set: frames {start_frame}..{end_frame}"
                ));
            }
            PlaybackCommand::ClearLoop => {
                self.clear_loop_internal();
                dev::log("loop cleared");
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

    fn clear_queued_next(&self) {
        *self.shared.next_track.lock().expect("next track lock") = None;
    }

    fn clear_loop_internal(&self) {
        self.shared.loop_active.store(false, Ordering::Relaxed);
        self.shared.loop_start_frame.store(0, Ordering::Relaxed);
        self.shared.loop_end_frame.store(0, Ordering::Relaxed);
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

    if !shared.playing.load(Ordering::Relaxed) || out_frames == 0 {
        return;
    }

    let Ok(mut eqs) = shared.eq.try_lock() else {
        return;
    };

    let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
    let gains = read_gains(shared);
    let mut out_frame = 0usize;

    while out_frame < out_frames {
        let Some((samples, src_channels, duration_frames)) = read_current_track(shared) else {
            shared.playing.store(false, Ordering::Relaxed);
            status.playing.store(false, Ordering::Relaxed);
            return;
        };

        let src_channels = src_channels.max(1);
        let mut frame = shared.playhead_frame.load(Ordering::Relaxed);

        if frame >= duration_frames && duration_frames > 0 {
            if try_swap_to_queued_track(shared, &mut eqs, status) {
                continue;
            }
            shared.playing.store(false, Ordering::Relaxed);
            status.playing.store(false, Ordering::Relaxed);
            return;
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
        if shared.loop_active.load(Ordering::Relaxed) {
            let loop_start = shared.loop_start_frame.load(Ordering::Relaxed);
            let loop_end = shared.loop_end_frame.load(Ordering::Relaxed);
            if loop_end > loop_start && frame >= loop_end {
                frame = loop_start;
                for eq in eqs.iter_mut() {
                    eq.reset(shared.output_sample_rate);
                }
            }
        }
        out_frame += 1;
        shared.playhead_frame.store(frame, Ordering::Relaxed);
        status.playhead_frame.store(frame, Ordering::Relaxed);

        if frame >= duration_frames && duration_frames > 0 {
            if try_swap_to_queued_track(shared, &mut eqs, status) {
                continue;
            }
            shared.playing.store(false, Ordering::Relaxed);
            status.playing.store(false, Ordering::Relaxed);
            return;
        }
    }
}

fn read_current_track(shared: &SharedPlayback) -> Option<(Arc<[f32]>, usize, u64)> {
    let guard = shared.track.try_lock().ok()?;
    guard.as_ref().map(|t| {
        (
            Arc::clone(&t.samples),
            t.channels,
            t.duration_frames,
        )
    })
}

/// Swap the preloaded `next_track` into `track` at end-of-song — keeps `playing` true.
fn try_swap_to_queued_track(
    shared: &SharedPlayback,
    eqs: &mut [LiveEq],
    status: &PlaybackStatus,
) -> bool {
    let Ok(mut next_guard) = shared.next_track.try_lock() else {
        return false;
    };
    let Some(next) = next_guard.take() else {
        return false;
    };
    let Ok(mut track_guard) = shared.track.try_lock() else {
        *next_guard = Some(next);
        return false;
    };

    let duration_frames = next.duration_frames;
    *track_guard = Some(next);
    shared.playhead_frame.store(0, Ordering::Relaxed);
    shared.duration_frames.store(duration_frames, Ordering::Relaxed);
    status.playhead_frame.store(0, Ordering::Relaxed);
    status.duration_frames.store(duration_frames, Ordering::Relaxed);
    shared.auto_advanced.store(true, Ordering::Relaxed);

    for eq in eqs.iter_mut() {
        eq.reset(shared.output_sample_rate);
    }

    true
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

#[cfg(test)]
mod stream_event_tests {
    use super::*;

    #[test]
    fn events_are_taken_once_and_lost_outranks_changed() {
        let engine = PlaybackEngine::silent();
        assert_eq!(engine.take_stream_event(), StreamEvent::None);

        engine.shared.stream_event.fetch_max(StreamEvent::CHANGED, Ordering::Relaxed);
        assert_eq!(engine.take_stream_event(), StreamEvent::DeviceChanged);
        assert_eq!(engine.take_stream_event(), StreamEvent::None);

        engine.shared.stream_event.fetch_max(StreamEvent::LOST, Ordering::Relaxed);
        engine.shared.stream_event.fetch_max(StreamEvent::CHANGED, Ordering::Relaxed);
        assert_eq!(engine.take_stream_event(), StreamEvent::DeviceLost);
    }

    #[test]
    fn drop_output_stops_playing_and_refuses_play() {
        let mut engine = PlaybackEngine::silent();
        engine.shared.playing.store(true, Ordering::Relaxed);
        engine.drop_output();
        assert!(!engine.has_output());
        assert!(!engine.shared.playing.load(Ordering::Relaxed));
        assert!(engine.play().is_err());
    }
}
