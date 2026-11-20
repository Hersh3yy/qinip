//! App state and everything that is not drawing.
//!
//! This wires the `qinip` library the way the egui app (src/app.rs) does:
//! - `Playlist::append_folder` adds tracks; the pending ones are queued on the
//!   `AnalysisWorker`, which decodes each file and computes the four-band waveform peaks off
//!   the UI thread.
//! - On `WorkerUpdate::TrackReady` the peaks and the decoded audio are kept, and a thread
//!   runs `detect_bpm` + `detect_key`; the result lands in `TrackEntry::set_analysis_result`.
//! - Playback goes through `PlaybackEngine`; its status is mirrored into plain fields once
//!   per frame so the view only reads numbers.
//!
//! Rune draws only when something wakes it, so background threads call the `Waker` Rune
//! hands over in `setup`, and `tick` (run before every frame) drains the channels.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use qinip::bpm::detect_bpm;
use qinip::decode::{DecodedAudio, decode_to_samples};
use qinip::eq::{BandWaveform, EqGains, band_waveform_peaks};
use qinip::key::detect_key;
use qinip::playback::PlaybackEngine;
use qinip::playlist::{AnalysisState, Playlist, TrackEntry};
use qinip::theme::ThemeChoice;
use qinip::worker::{AnalysisWorker, WorkerJob, WorkerUpdate};
use rune::Waker;
use rune_kit::{Kit, TableState};

use crate::colors::{self, Ink};

/// Waveform resolution, as in src/worker.rs (used by the synchronous headless scenes).
const WAVEFORM_PEAKS: usize = 800;

/// One finished BPM/key analysis: `(bpm, beat offset, key name)`, or why it failed.
pub struct AnalysisResult {
    pub path: PathBuf,
    pub outcome: Result<(f32, f32, String), String>,
}

pub struct App {
    pub theme: ThemeChoice,
    pub ink: Ink,
    pub kit: Kit,
    pub playlist: Playlist,
    /// rune-kit's table keeps the selection, keyboard cursor, sort and column widths here.
    pub table: TableState,
    pub playing_track: Option<usize>,
    pub volume: f32,
    /// The four EQ knobs, 0.0..=2.0 each (1.0 = unity), applied in the audio path.
    pub eq: EqGains,
    /// Playback status mirrored once per frame by `tick`.
    pub position_secs: f64,
    pub duration_secs: f64,
    pub is_playing: bool,
    pub status_message: String,
    /// A file drag from another app is over the window.
    pub drop_over: bool,

    audio_cache: HashMap<PathBuf, Arc<DecodedAudio>>,
    waveform_cache: HashMap<PathBuf, BandWaveform>,
    playback: PlaybackEngine,
    worker: AnalysisWorker,
    /// The worker's own channel, until `attach_waker` hands it to the forwarding thread.
    worker_rx: Option<Receiver<WorkerUpdate>>,
    /// Worker updates, forwarded by a thread that also wakes Rune.
    updates_tx: Sender<WorkerUpdate>,
    updates_rx: Receiver<WorkerUpdate>,
    analysis_tx: Sender<AnalysisResult>,
    analysis_rx: Receiver<AnalysisResult>,
    /// The track the user asked to play that is still decoding.
    pending_play: Option<PathBuf>,
    waker: Option<Waker>,
    /// Headless "playing" scene only: a fixed playhead, since no audio device runs there.
    preview_position: Option<f64>,
}

impl App {
    pub fn new() -> App {
        let headless = rune::tooling::headless_run().is_some();
        let playback = if headless {
            PlaybackEngine::silent()
        } else {
            PlaybackEngine::new().unwrap_or_else(|err| {
                eprintln!("no audio output ({err}); running silent");
                PlaybackEngine::silent()
            })
        };
        let (worker, worker_rx) = AnalysisWorker::spawn();
        let (updates_tx, updates_rx) = mpsc::channel();
        let (analysis_tx, analysis_rx) = mpsc::channel();
        let theme = ThemeChoice::Dark;
        let ink = Ink::from_palette(&theme.palette(None));
        let kit = colors::kit_for(&ink);
        let mut app = App {
            theme,
            ink,
            kit,
            playlist: Playlist::new(),
            table: TableState::new().key("playlist"),
            playing_track: None,
            volume: 0.8,
            eq: EqGains::default(),
            position_secs: 0.0,
            duration_secs: 0.0,
            is_playing: false,
            status_message: "Load a folder of MP3s to start.".to_string(),
            drop_over: false,
            audio_cache: HashMap::new(),
            waveform_cache: HashMap::new(),
            playback,
            worker,
            worker_rx: Some(worker_rx),
            updates_tx,
            updates_rx,
            analysis_tx,
            analysis_rx,
            pending_play: None,
            waker: None,
            preview_position: None,
        };
        let _ = app.playback.set_volume(app.volume);
        let _ = app.playback.set_eq(app.eq);
        // `cargo run -p qinip-rune -- /folder` loads that folder on launch. Headless scenes
        // load synchronously instead (`scene_load_sync`), so a snapshot is deterministic.
        if !headless
            && let Some(folder) = rune::extra_args().first().map(PathBuf::from).filter(|p| p.is_dir())
        {
            app.load_folder(folder);
        }
        app
    }

    /// Rune's `setup` hook gives a waker; background threads call it to ask for a frame.
    pub fn attach_waker(&mut self, waker: Waker) {
        self.waker = Some(waker.clone());
        if let Some(rx) = self.worker_rx.take() {
            let tx = self.updates_tx.clone();
            thread::spawn(move || {
                for update in rx {
                    if tx.send(update).is_err() {
                        break;
                    }
                    waker();
                }
            });
        }
    }

    pub fn set_theme(&mut self, theme: ThemeChoice) {
        self.theme = theme;
        self.ink = Ink::from_palette(&theme.palette(None));
        self.kit = colors::kit_for(&self.ink);
    }

    pub fn set_theme_by_label(&mut self, label: &str) {
        if let Some(theme) = ThemeChoice::ALL.iter().copied().find(|t| t.label() == label) {
            self.set_theme(theme);
        }
    }

    // ---- Loading ---------------------------------------------------------------------------

    pub fn load_folder(&mut self, folder: PathBuf) {
        let added = self.playlist.append_folder(&folder);
        self.status_message = if added == 0 {
            format!("No MP3s found in {}", folder.display())
        } else {
            format!("Added {added} track{} from {}", plural(added), folder.display())
        };
        self.analyze_pending();
    }

    pub fn load_paths(&mut self, paths: Vec<PathBuf>) {
        let added = self.playlist.append_paths(paths);
        self.status_message = if added == 0 {
            "Nothing added: only MP3 files can be loaded.".to_string()
        } else {
            format!("Added {added} track{}", plural(added))
        };
        self.analyze_pending();
    }

    /// Files or folders dropped on the window: folders are walked, loose MP3s are added.
    pub fn load_dropped(&mut self, files: &[PathBuf]) {
        let (folders, loose): (Vec<PathBuf>, Vec<PathBuf>) = files.iter().cloned().partition(|p| p.is_dir());
        for folder in folders {
            self.load_folder(folder);
        }
        if !loose.is_empty() {
            self.load_paths(loose);
        }
    }

    pub fn clear_playlist(&mut self) {
        self.playlist = Playlist::new();
        self.table = TableState::new().key("playlist");
        self.playing_track = None;
        self.pending_play = None;
        self.preview_position = None;
        self.audio_cache.clear();
        self.waveform_cache.clear();
        let _ = self.playback.unload();
        self.status_message = "Playlist cleared.".to_string();
    }

    /// Queue every track that still needs a decode (as `analyze_pending` in app.rs).
    fn analyze_pending(&mut self) {
        let mut count = 0;
        for track in &mut self.playlist.tracks {
            if self.audio_cache.contains_key(&track.path) {
                continue;
            }
            if matches!(track.analysis, AnalysisState::Pending | AnalysisState::Failed(_)) {
                track.analysis = AnalysisState::Loading;
                self.worker.enqueue(WorkerJob::LoadTrack { path: track.path.clone(), autoplay: false });
                count += 1;
            }
        }
        if count > 0 {
            self.status_message =
                format!("{} Analyzing {count} track{} in the background…", self.status_message, plural(count));
        }
    }

    // ---- Per frame -------------------------------------------------------------------------

    /// Runs before every frame: apply playback commands, take what the threads sent, mirror
    /// the playback status. Returns true when the view should re-run.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        self.playback.poll();

        // Updates that arrived before `attach_waker` (or in a headless run) are still on the
        // worker's own channel.
        if let Some(rx) = &self.worker_rx {
            while let Ok(update) = rx.try_recv() {
                let _ = self.updates_tx.send(update);
            }
        }
        while let Ok(update) = self.updates_rx.try_recv() {
            self.apply_update(update);
            changed = true;
        }
        while let Ok(result) = self.analysis_rx.try_recv() {
            self.apply_analysis(result);
            changed = true;
        }

        let status = self.playback.status();
        let (position, duration, playing) = match self.preview_position {
            Some(p) => (p, self.track_duration(self.playing_track), true),
            None => (status.position_secs(), status.duration_secs(), status.is_playing()),
        };
        if (position - self.position_secs).abs() > 1e-6 || duration != self.duration_secs || playing != self.is_playing {
            changed = true;
        }
        // The engine stops at the end of a track; move on to the next one, as autoplay does.
        let ended = self.is_playing
            && !playing
            && self.playing_track.is_some()
            && duration > 0.0
            && position >= duration - 0.05
            && self.preview_position.is_none();
        self.position_secs = position;
        self.duration_secs = duration;
        self.is_playing = playing;
        if ended {
            self.next();
        }
        changed
    }

    fn apply_update(&mut self, update: WorkerUpdate) {
        match update {
            WorkerUpdate::TrackReady { path, audio, peaks, autoplay: _ } => {
                let Some(index) = self.playlist.index_of(&path) else { return };
                self.waveform_cache.insert(path.clone(), peaks);
                // Tracks whose BPM and key came from their tags are already Done; this decode
                // was for the waveform and playback only.
                let needs_analysis = self.playlist.tracks[index].analysis != AnalysisState::Done;
                {
                    let track = &mut self.playlist.tracks[index];
                    track.duration_secs = Some(audio.duration_secs());
                    if needs_analysis {
                        track.analysis = AnalysisState::Analyzing;
                    }
                }
                let audio = Arc::new(audio);
                self.audio_cache.insert(path.clone(), Arc::clone(&audio));
                if needs_analysis {
                    self.spawn_analysis(path.clone(), audio);
                }
                // Only the track the user asked for last starts playing.
                if self.pending_play.as_ref() == Some(&path) {
                    self.pending_play = None;
                    self.play_track(index);
                }
            }
            WorkerUpdate::Failed { path, message } => {
                if let Some(index) = self.playlist.index_of(&path) {
                    self.playlist.tracks[index].analysis = AnalysisState::Failed(message.clone());
                }
                if self.pending_play.as_ref() == Some(&path) {
                    self.pending_play = None;
                }
                self.status_message = format!("{} failed: {message}", path.display());
            }
        }
    }

    /// BPM + key on a thread, panic-guarded as app.rs does, then wake Rune for the result.
    fn spawn_analysis(&self, path: PathBuf, audio: Arc<DecodedAudio>) {
        let tx = self.analysis_tx.clone();
        let waker = self.waker.clone();
        thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let bpm = detect_bpm(&audio);
                let key = detect_key(&audio);
                (bpm.bpm, bpm.beat_offset_secs, key.key_name)
            }))
            .map_err(|_| "BPM/key analysis panicked".to_string());
            let _ = tx.send(AnalysisResult { path, outcome });
            if let Some(wake) = waker {
                wake();
            }
        });
    }

    fn apply_analysis(&mut self, result: AnalysisResult) {
        let Some(index) = self.playlist.index_of(&result.path) else { return };
        let track = &mut self.playlist.tracks[index];
        match result.outcome {
            Ok((bpm, beat_offset, key)) => track.set_analysis_result(bpm, beat_offset, key),
            Err(message) => track.analysis = AnalysisState::Failed(message),
        }
        self.prune_audio_cache();
    }

    /// Decoded audio is about 100 MB a track: keep only what plays, what was asked for and
    /// what is still being analysed (as `prune_audio_cache` in app.rs).
    fn prune_audio_cache(&mut self) {
        let mut keep: Vec<PathBuf> = Vec::new();
        if let Some(track) = self.playing_track.and_then(|i| self.playlist.tracks.get(i)) {
            keep.push(track.path.clone());
        }
        keep.extend(self.pending_play.iter().cloned());
        keep.extend(
            self.playlist
                .tracks
                .iter()
                .filter(|t| matches!(t.analysis, AnalysisState::Loading | AnalysisState::Analyzing))
                .map(|t| t.path.clone()),
        );
        self.audio_cache.retain(|path, _| keep.contains(path));
    }

    // ---- Right-click actions ---------------------------------------------------------------

    /// A pick from the row's context menu (ids from `view::track_menu`).
    pub fn track_action(&mut self, index: usize, action: &str) {
        match action {
            "play" => self.request_play(index),
            "remove" => self.remove_track(index),
            "folder" => self.open_enclosing_folder(index),
            "tags" => self.write_tags(index),
            "reanalyze" => self.reanalyze(index),
            _ => {}
        }
    }

    /// Drop a row from the list (the file stays on disk), keeping the playing index right.
    fn remove_track(&mut self, index: usize) {
        let Some(removed) = self.playlist.remove_at(index) else { return };
        self.audio_cache.remove(&removed.path);
        self.waveform_cache.remove(&removed.path);
        if self.pending_play.as_ref() == Some(&removed.path) {
            self.pending_play = None;
        }
        match self.playing_track {
            Some(playing) if playing == index => {
                let _ = self.playback.unload();
                self.playing_track = None;
            }
            Some(playing) if playing > index => self.playing_track = Some(playing - 1),
            _ => {}
        }
        self.status_message = format!("Removed {} from playlist", removed.display_label());
    }

    fn open_enclosing_folder(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get(index) else { return };
        self.status_message = match qinip::file_ops::open_enclosing_folder(&track.path) {
            Ok(()) => format!("Opened {}", track.filename_label()),
            Err(err) => err,
        };
    }

    /// Fill missing TBPM/TKEY in the file (never overwrites), as the egui app's menu does.
    fn write_tags(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get(index) else { return };
        let label = track.display_label();
        let (Some(bpm), Some(key)) = (track.bpm, track.key.clone()) else {
            self.status_message = "Write tags needs BPM and Key: wait for analysis to finish.".to_string();
            return;
        };
        self.status_message = match qinip::tags::write_analysis_tags(&track.path, bpm, &key) {
            Ok(true) => format!("Wrote missing BPM/Key tags to {label}"),
            Ok(false) => format!("BPM/Key tags already present on {label}, left intact"),
            Err(err) => format!("Tag write failed: {err}"),
        };
    }

    /// Forget the tagged or earlier values and let Qinip listen again.
    fn reanalyze(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get_mut(index) else { return };
        track.mark_for_reanalysis();
        let path = track.path.clone();
        let label = track.display_label();
        if let Some(audio) = self.audio_cache.get(&path).map(Arc::clone) {
            self.playlist.tracks[index].analysis = AnalysisState::Analyzing;
            self.spawn_analysis(path, audio);
        } else {
            self.playlist.tracks[index].analysis = AnalysisState::Loading;
            self.worker.enqueue(WorkerJob::LoadTrack { path, autoplay: false });
        }
        self.status_message = format!("Re-analyzing {label}…");
    }

    // ---- Playback --------------------------------------------------------------------------

    /// The playlist index of a table row id (the track's path).
    pub fn index_of_id(&self, id: &str) -> Option<usize> {
        self.playlist.index_of(Path::new(id))
    }

    /// Play a track: at once when its audio is decoded, else after the worker decodes it
    /// (play requests jump the worker's queue).
    pub fn request_play(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get(index) else { return };
        let path = track.path.clone();
        self.table.select(path.to_string_lossy().into_owned());
        if self.audio_cache.contains_key(&path) {
            self.play_track(index);
            return;
        }
        let track = &mut self.playlist.tracks[index];
        if track.analysis != AnalysisState::Done && track.analysis != AnalysisState::Analyzing {
            track.analysis = AnalysisState::Loading;
        }
        self.status_message = format!("Loading {}…", clean(&track.display_label()));
        self.pending_play = Some(path.clone());
        self.worker.enqueue(WorkerJob::LoadTrack { path, autoplay: true });
    }

    fn play_track(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get(index) else { return };
        let Some(audio) = self.audio_cache.get(&track.path).map(Arc::clone) else { return };
        let label = clean(&track.display_label());
        let _ = self.playback.load(audio);
        let _ = self.playback.seek_secs(0.0);
        let _ = self.playback.set_volume(self.volume);
        let _ = self.playback.play();
        self.playing_track = Some(index);
        self.pending_play = None;
        self.status_message = format!("Playing {label}");
        self.prune_audio_cache();
    }

    /// Play/Pause: pause what plays, resume what is paused, else play the selected track.
    pub fn toggle_play(&mut self) {
        if self.is_playing {
            let _ = self.playback.pause();
            return;
        }
        if self.playing_track.is_some() && self.duration_secs > 0.0 {
            let _ = self.playback.play();
            return;
        }
        let candidate = self.selected_index().or_else(|| (!self.playlist.tracks.is_empty()).then_some(0));
        if let Some(index) = candidate {
            self.request_play(index);
        }
    }

    pub fn stop(&mut self) {
        let _ = self.playback.pause();
        let _ = self.playback.seek_secs(0.0);
        self.position_secs = 0.0;
    }

    pub fn next(&mut self) {
        let Some(current) = self.playing_track.or_else(|| self.selected_index()) else { return };
        if current + 1 < self.playlist.tracks.len() {
            self.request_play(current + 1);
        } else {
            let _ = self.playback.pause();
            self.playing_track = None;
            self.status_message = "Playlist finished.".to_string();
        }
    }

    /// Previous: restart the track past its first seconds, else go to the one before.
    pub fn previous(&mut self) {
        let Some(current) = self.playing_track.or_else(|| self.selected_index()) else { return };
        if self.position_secs > 3.0 || current == 0 {
            let _ = self.playback.seek_secs(0.0);
            self.position_secs = 0.0;
        } else {
            self.request_play(current - 1);
        }
    }

    /// Seek to a fraction (0..1) of the track; the waveform and the seek bar both call this.
    pub fn seek_fraction(&mut self, fraction: f32) {
        if self.duration_secs <= 0.0 {
            return;
        }
        let secs = f64::from(fraction.clamp(0.0, 1.0)) * self.duration_secs;
        let _ = self.playback.seek_secs(secs);
        self.position_secs = secs;
        if self.preview_position.is_some() {
            self.preview_position = Some(secs);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        let _ = self.playback.set_volume(self.volume);
    }

    /// One EQ knob (0 = Low, 1 = LowMid, 2 = HiMid, 3 = Hi) to `gain`, 0..=2.
    pub fn set_eq_band(&mut self, band: usize, gain: f32) {
        let gain = gain.clamp(0.0, 2.0);
        match band {
            0 => self.eq.low = gain,
            1 => self.eq.low_mid = gain,
            2 => self.eq.hi_mid = gain,
            _ => self.eq.hi = gain,
        }
        let _ = self.playback.set_eq(self.eq);
    }

    pub fn eq_band(&self, band: usize) -> f32 {
        match band {
            0 => self.eq.low,
            1 => self.eq.low_mid,
            2 => self.eq.hi_mid,
            _ => self.eq.hi,
        }
    }

    pub fn reset_eq(&mut self) {
        self.eq = EqGains::default();
        let _ = self.playback.set_eq(self.eq);
    }

    // ---- Reads for the view ----------------------------------------------------------------

    /// A play request or a decode/analysis is still outstanding on a background thread.
    pub fn has_background_work(&self) -> bool {
        self.pending_play.is_some()
            || self.playlist.tracks.iter().any(|t| matches!(t.analysis, AnalysisState::Loading | AnalysisState::Analyzing))
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.table.cursor().and_then(|id| self.index_of_id(id))
    }

    /// The track the waveform shows: what plays, else what is selected.
    pub fn current_track(&self) -> Option<&TrackEntry> {
        self.playing_track.or_else(|| self.selected_index()).and_then(|i| self.playlist.tracks.get(i))
    }

    pub fn current_waveform(&self) -> Option<&BandWaveform> {
        self.current_track().and_then(|t| self.waveform_cache.get(&t.path))
    }

    pub fn current_bpm(&self) -> Option<f32> {
        self.playing_track.and_then(|i| self.playlist.tracks.get(i)).and_then(|t| t.bpm)
    }

    fn track_duration(&self, index: Option<usize>) -> f64 {
        index.and_then(|i| self.playlist.tracks.get(i)).and_then(|t| t.duration_secs).unwrap_or(0.0)
    }

    // ---- Headless scenes (`--scene loaded`, `--scene playing`) -----------------------------

    /// Load the folder from the command line (or `QINIP_RUNE_FOLDER`) and decode and analyse
    /// every track on this thread, so a headless snapshot shows the finished playlist.
    pub fn scene_load_sync(&mut self) {
        let folder = scene_folder();
        let added = self.playlist.append_folder(&folder);
        self.status_message = format!("Added {added} track{} from {}", plural(added), folder.display());
        for track in &mut self.playlist.tracks {
            let path = track.path.clone();
            let audio = match decode_to_samples(&path.to_string_lossy()) {
                Ok(audio) => audio,
                Err(message) => {
                    track.analysis = AnalysisState::Failed(message);
                    continue;
                }
            };
            let peaks = band_waveform_peaks(&audio.downmix_to_mono(), audio.sample_rate, WAVEFORM_PEAKS);
            track.duration_secs = Some(audio.duration_secs());
            self.waveform_cache.insert(path.clone(), peaks);
            if track.analysis != AnalysisState::Done {
                let bpm = detect_bpm(&audio);
                let key = detect_key(&audio);
                track.set_analysis_result(bpm.bpm, bpm.beat_offset_secs, key.key_name);
            }
            self.audio_cache.insert(path, Arc::new(audio));
        }
    }

    /// Pretend the second track plays, a third of the way in (no audio device headless).
    pub fn scene_play_preview(&mut self) {
        let index = 1.min(self.playlist.tracks.len().saturating_sub(1));
        let Some(track) = self.playlist.tracks.get(index) else { return };
        let label = clean(&track.display_label());
        self.table.select(track.path.to_string_lossy().into_owned());
        self.playing_track = Some(index);
        self.preview_position = Some(track.duration_secs.unwrap_or(0.0) * 0.37);
        self.status_message = format!("Playing {label}");
    }
}

/// Tag text sometimes starts with a byte-order mark (U+FEFF); keep it out of messages.
pub fn clean(text: &str) -> String {
    text.replace('\u{feff}', "")
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The folder a headless scene loads: the first extra argument, else `QINIP_RUNE_FOLDER`.
fn scene_folder() -> PathBuf {
    rune::extra_args()
        .first()
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("QINIP_RUNE_FOLDER").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../test_audio_files")))
}
