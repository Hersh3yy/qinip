use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;

use crate::bpm::detect_bpm;
use crate::decode::DecodedAudio;
use crate::dev;
use crate::eq::{band_waveform_peaks, BandWaveform, EqGains, BAND_COLORS, BAND_LABELS};
use crate::file_ops;
use crate::key::detect_key;
use crate::knob::eq_knob;
use crate::media::SystemMedia;
use crate::selection::Selection;
use crate::playback::PlaybackEngine;
use crate::playlist::{AnalysisState, Playlist, SortColumn, SortDirection};
use crate::tags;
use crate::theme::{Palette, ThemeChoice};
use crate::transport::{beat_bar_indicator, transport_button};
use crate::worker::{AnalysisWorker, WorkerJob, WorkerUpdate};
use souvlaki::MediaControlEvent;

struct AnalysisResult {
    path: PathBuf,
    /// `Ok((bpm, key_name))` on success, `Err(message)` if detection failed
    /// or panicked — either way the UI shows it, it never just hangs on
    /// "Analyzing…" forever.
    outcome: Result<(f32, String), String>,
}

/// Resizable playlist column widths (Title column takes remaining space).
#[derive(Clone)]
struct PlaylistColumns {
    track_num: f32,
    artist: f32,
    title: f32,
    duration: f32,
    bpm: f32,
    key: f32,
    camelot: f32,
    played: f32,
}

impl Default for PlaylistColumns {
    fn default() -> Self {
        Self {
            track_num: 36.0,
            artist: 160.0,
            title: 220.0,
            duration: 56.0,
            bpm: 48.0,
            key: 80.0,
            camelot: 64.0,
            played: 52.0,
        }
    }
}

impl PlaylistColumns {
    fn fixed_width(&self) -> f32 {
        self.track_num + self.artist + self.duration + self.bpm + self.key + self.camelot + self.played
    }

    fn title_width(&self, avail: f32, spacing: f32) -> f32 {
        (avail - self.fixed_width() - spacing * 7.0).max(self.title)
    }
}

/// User preferences that persist across launches (eframe storage, under
/// ~/Library/Application Support/Qinip on macOS, %APPDATA%\Qinip on Windows).
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Settings {
    volume: f32,
    autoplay: bool,
    /// Opt-in: write analyzed BPM/Key into the MP3's ID3 tags automatically.
    /// Off by default so Qinip never edits someone's files unasked.
    auto_write_tags: bool,
    theme: ThemeChoice,
    repeat: RepeatMode,
    shuffle: bool,
}

/// What happens when the playing track ends (and what Next does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum RepeatMode {
    /// Stop after the last track.
    #[default]
    Off,
    /// Wrap from the last track back to the first.
    All,
    /// Play the same track again (manual Next still moves on).
    One,
}

impl RepeatMode {
    fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

    fn label(self) -> &'static str {
        match self {
            RepeatMode::Off => "Repeat off",
            RepeatMode::All => "Repeat all",
            RepeatMode::One => "Repeat one",
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 0.85,
            autoplay: true,
            auto_write_tags: false,
            theme: ThemeChoice::System,
            repeat: RepeatMode::Off,
            shuffle: false,
        }
    }
}

const SETTINGS_KEY: &str = "qinip-settings";

pub struct QinipApp {
    playlist: Playlist,
    playback: PlaybackEngine,
    worker: AnalysisWorker,
    updates_rx: Receiver<WorkerUpdate>,
    analysis_tx: Sender<AnalysisResult>,
    analysis_rx: Receiver<AnalysisResult>,
    audio_cache: HashMap<PathBuf, Arc<DecodedAudio>>,
    waveform_cache: HashMap<PathBuf, BandWaveform>,
    /// Anchor row + multi-select set. `selected()` gives the anchor.
    selection: Selection,
    playing_track: Option<usize>,
    volume: f32,
    /// Volume to restore when un-muting via the speaker button.
    volume_before_mute: f32,
    eq: EqGains,
    seek_slider: f64,
    status_message: String,
    autoplay: bool,
    auto_write_tags: bool,
    show_about: bool,
    theme: ThemeChoice,
    /// Colours for this frame (resolved from `theme` + the OS setting).
    palette: Palette,
    /// Last palette pushed into egui, so visuals are only rebuilt on change.
    applied_palette: Option<Palette>,
    repeat: RepeatMode,
    shuffle: bool,
    /// The track Next/autoplay will go to, chosen once so the gapless preload
    /// and the actual advance agree (matters for shuffle). Stored by path so
    /// it survives sorting; cleared whenever the queue context changes.
    upcoming: Option<PathBuf>,
    /// Tracks played since shuffle went on: avoids repeats until every track
    /// had a turn, and lets Previous walk back through the shuffle order.
    history: Vec<PathBuf>,
    was_playing: bool,
    last_listen_at: Option<f64>,
    /// When set, show a confirm dialog before deleting this playlist index from disk.
    confirm_delete: Option<usize>,
    playlist_sort: Option<(SortColumn, SortDirection)>,
    playlist_columns: PlaylistColumns,
    /// True once we've queued the upcoming track for gapless autoplay.
    autoplay_preloaded: bool,
    /// `None` if the OS media-controls integration failed to start; the app
    /// works fine without it (no media keys / Now Playing).
    system_media: Option<SystemMedia>,
    /// Set when there is no usable audio output; drives the banner + Retry.
    audio_error: Option<String>,
    /// Active beat loop size (2/4/8/16), if any.
    loop_beats: Option<u32>,
    loop_start_secs: f64,
    loop_end_secs: f64,
}

impl QinipApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if dev::enabled() {
            eprintln!("[dev] QINIP_DEV enabled — logging to stderr");
            eprintln!("[dev] tip: use `cargo run --release` for ~18x faster analysis");
        }


        crate::macos_icon::set_dock_icon();

        // Never crash at launch over audio: a Mac with no output device (or a
        // Bluetooth device mid-switch) gets a silent engine and a Retry banner.
        let (playback, audio_error) = match PlaybackEngine::new() {
            Ok(engine) => (engine, None),
            Err(err) => {
                dev::log(format!("audio output failed: {err}"));
                (PlaybackEngine::silent(), Some(err.to_string()))
            }
        };
        let system_media = match SystemMedia::new(cc.egui_ctx.clone()) {
            Ok(media) => Some(media),
            Err(err) => {
                dev::log(format!("system media controls unavailable: {err}"));
                None
            }
        };
        let settings: Settings = cc
            .storage
            .and_then(|storage| eframe::get_value(storage, SETTINGS_KEY))
            .unwrap_or_default();
        let volume = settings.volume.clamp(0.0, 1.0);
        let _ = playback.set_volume(volume);

        let (worker, updates_rx) = AnalysisWorker::spawn();
        let (analysis_tx, analysis_rx) = mpsc::channel();

        Self {
            playlist: Playlist::new(),
            playback,
            worker,
            updates_rx,
            analysis_tx,
            analysis_rx,
            audio_cache: HashMap::new(),
            waveform_cache: HashMap::new(),
            selection: Selection::default(),
            playing_track: None,
            volume,
            volume_before_mute: volume,
            eq: EqGains::default(),
            seek_slider: 0.0,
            status_message: "Load a folder or drop MP3s to begin.".to_string(),
            autoplay: settings.autoplay,
            auto_write_tags: settings.auto_write_tags,
            show_about: false,
            theme: settings.theme,
            palette: settings.theme.palette(system_dark(&cc.egui_ctx)),
            applied_palette: None,
            repeat: settings.repeat,
            shuffle: settings.shuffle,
            upcoming: None,
            history: Vec::new(),
            was_playing: false,
            last_listen_at: None,
            confirm_delete: None,
            playlist_sort: None,
            playlist_columns: PlaylistColumns::default(),
            autoplay_preloaded: false,
            system_media,
            audio_error,
            loop_beats: None,
            loop_start_secs: 0.0,
            loop_end_secs: 0.0,
        }
    }

    fn poll_worker(&mut self) {
        while let Ok(update) = self.updates_rx.try_recv() {
            match update {
                WorkerUpdate::TrackReady {
                    path,
                    audio,
                    autoplay,
                } => {
                    let Some(track_index) = self.playlist.index_of(&path) else {
                        continue;
                    };

                    dev::log(format!(
                        "track ready {} — {:.1}s",
                        path.display(),
                        audio.duration_secs()
                    ));

                    let peak_start = std::time::Instant::now();
                    let mono = audio.downmix_to_mono();
                    let peaks = band_waveform_peaks(&mono, audio.sample_rate, 800);
                    self.waveform_cache.insert(path.clone(), peaks);
                    dev::log_timing("band waveform peaks", peak_start);

                    if let Some(track) = self.playlist.tracks.get_mut(track_index) {
                        track.analysis = AnalysisState::Analyzing;
                        track.duration_secs = Some(audio.duration_secs());
                    }

                    let audio = Arc::new(audio);
                    self.audio_cache.insert(path.clone(), Arc::clone(&audio));
                    self.spawn_analysis(path.clone(), audio);

                    if autoplay {
                        self.play_track(track_index, true);
                    }
                }
                WorkerUpdate::Failed { path, message } => {
                    if let Some(track_index) = self.playlist.index_of(&path)
                        && let Some(track) = self.playlist.tracks.get_mut(track_index)
                    {
                        track.analysis = AnalysisState::Failed(message.clone());
                    }
                    self.status_message = format!("{} failed: {message}", path.display());
                    dev::log(format!("track failed {}: {message}", path.display()));
                }
            }
        }

        while let Ok(result) = self.analysis_rx.try_recv() {
            self.apply_analysis_result(result);
        }
    }

    /// Update the matching playlist row with a finished analysis — success
    /// or failure, so a bad file never leaves the row stuck on "Analyzing…".
    fn apply_analysis_result(&mut self, result: AnalysisResult) {
        let Some(track_index) = self.playlist.index_of(&result.path) else {
            return;
        };
        let Some(track) = self.playlist.tracks.get_mut(track_index) else {
            return;
        };

        match result.outcome {
            Ok((bpm, key)) => {
                dev::log(format!(
                    "analysis done {} — {:.1} BPM, {key}",
                    result.path.display(),
                    bpm
                ));
                track.set_analysis_result(bpm, key);
            }
            Err(message) => {
                dev::log(format!("analysis failed {}: {message}", result.path.display()));
                track.analysis = AnalysisState::Failed(message);
            }
        }
    }

    /// Runs BPM + key detection on a background thread and reports the
    /// result back over `analysis_tx`. Wrapped in `catch_unwind` — a bad file
    /// tripping a panic inside `detect_bpm`/`detect_key` (see e.g. aubio's
    /// `.expect()` calls) must only fail that one track, not take down the
    /// thread silently or, worse, the whole app.
    fn spawn_analysis(&self, path: PathBuf, audio: Arc<DecodedAudio>) {
        let tx = self.analysis_tx.clone();
        // Only touch the file when the user opted in (top bar checkbox).
        let write_tags = self.auto_write_tags;
        thread::spawn(move || {
            let start = std::time::Instant::now();
            let outcome = run_bpm_key_analysis(&audio);
            dev::log_timing(&format!("bpm+key {}", path.display()), start);
            if write_tags && let Ok((bpm, ref key)) = outcome {
                match tags::write_analysis_tags(&path, bpm, key) {
                    Ok(true) => {
                        dev::log(format!("wrote missing BPM/Key tags: {}", path.display()));
                    }
                    Ok(false) => {
                        dev::log(format!(
                            "BPM/Key tags already present, left intact: {}",
                            path.display()
                        ));
                    }
                    Err(err) => {
                        dev::log(format!("tag write failed {}: {err}", path.display()));
                    }
                }
            }
            let _ = tx.send(AnalysisResult { path, outcome });
        });
    }

    fn load_folder(&mut self, folder: PathBuf) {
        self.forget_upcoming();
        let added = self.playlist.append_folder(&folder);
        self.status_message = format!(
            "Added {added} tracks from {} — analyzing…",
            folder.display()
        );
        dev::log(format!("folder appended ({added}): {}", folder.display()));
        self.analyze_pending();
    }

    fn load_paths(&mut self, paths: Vec<PathBuf>) {
        self.forget_upcoming();
        let added = self.playlist.append_paths(paths);
        self.status_message = format!("Added {added} tracks — analyzing…");
        self.analyze_pending();
    }

    fn analyze_pending(&mut self) {
        let jobs: Vec<_> = self
            .playlist
            .tracks
            .iter()
            .filter(|track| !self.audio_cache.contains_key(&track.path))
            .filter(|track| {
                matches!(
                    track.analysis,
                    AnalysisState::Pending | AnalysisState::Failed(_)
                )
            })
            .map(|track| {
                (
                    track.path.clone(),
                    WorkerJob::LoadTrack {
                        path: track.path.clone(),
                        autoplay: false,
                    },
                )
            })
            .collect();

        for (path, _) in &jobs {
            if let Some(index) = self.playlist.index_of(path)
                && let Some(entry) = self.playlist.tracks.get_mut(index)
            {
                entry.analysis = AnalysisState::Loading;
            }
        }

        for (_, job) in jobs {
            self.worker.enqueue(job);
        }

        self.status_message = "Analyzing tracks in background…".to_string();
        dev::log("queued analyze-pending");
    }

    /// The focused (anchor) row — what single-row actions act on.
    fn selected(&self) -> Option<usize> {
        self.selection.anchor()
    }

    fn select_track(&mut self, index: usize) {
        self.selection.select_only(index);
        if let Some(track) = self.playlist.tracks.get(index) {
            self.status_message = format!("Selected {}", track.display_label());
            dev::log(format!("selected track #{index}"));
        }
    }

    /// Click on a row with the modifiers held at that moment:
    /// Shift extends a range from the anchor, Cmd/Ctrl toggles, plain selects only.
    fn click_track(&mut self, index: usize, modifiers: egui::Modifiers) {
        if modifiers.shift {
            self.selection.extend_to(index);
        } else if modifiers.command {
            self.selection.toggle(index);
        } else {
            self.select_track(index);
            return;
        }
        let n = self.selection.len();
        self.status_message = format!("{n} track{} selected", if n == 1 { "" } else { "s" });
    }

    fn select_all_tracks(&mut self) {
        let n = self.playlist.tracks.len();
        if n == 0 {
            return;
        }
        self.selection.select_all(n);
        self.status_message = format!("{n} tracks selected — Backspace removes them from the list");
        dev::log("select all");
    }

    fn request_play(&mut self, index: usize) {
        self.selection.select_only(index);

        if self
            .playlist
            .tracks
            .get(index)
            .is_some_and(|t| self.audio_cache.contains_key(&t.path))
        {
            self.play_track(index, true);
            return;
        }

        if let Some(track) = self.playlist.tracks.get_mut(index) {
            track.analysis = AnalysisState::Loading;
            let path = track.path.clone();
            let label = track.display_label();
            self.worker.enqueue(WorkerJob::LoadTrack {
                path,
                autoplay: true,
            });
            self.status_message = format!("Loading {label}…");
            dev::log(format!("request play #{index}: {label}"));
        }
    }

    fn play_track(&mut self, index: usize, restart: bool) {
        let Some(path) = self.playlist.tracks.get(index).map(|t| t.path.clone()) else {
            return;
        };
        // `audio_cache` holds one decoded copy per track; `Arc::clone` here just
        // shares that same buffer with the playback engine (cheap refcount bump),
        // rather than duplicating the multi-megabyte sample data on every play.
        let Some(audio) = self.audio_cache.get(&path).map(Arc::clone) else {
            dev::log(format!("play_track #{index} — not in cache yet"));
            return;
        };

        let _ = self.playback.load(audio);
        if restart {
            let _ = self.playback.seek_secs(0.0);
        }
        let _ = self.playback.set_volume(self.volume);
        let _ = self.playback.set_eq(self.eq);
        let _ = self.playback.play();

        self.playing_track = Some(index);
        self.selection.select_only(index);
        self.last_listen_at = None;
        self.autoplay_preloaded = false;
        self.upcoming = None;
        if self.shuffle && self.history.last() != Some(&path) {
            self.history.push(path.clone());
        }
        self.clear_loop();
        self.was_playing = self
            .playlist
            .tracks
            .get(index)
            .is_some_and(|t| t.qualifies_as_played());
        self.status_message = format!("Playing {}", self.playlist.tracks[index].display_label());
        dev::log(format!("playing track #{index}"));
        self.sync_system_media();
    }

    /// Next button / media key: always moves on, even in Repeat one.
    fn play_next(&mut self) {
        self.advance(true);
    }

    /// Go to the upcoming track. `manual` = the user asked (Next button);
    /// otherwise it's autoplay at the end of a track, where Repeat one replays.
    fn advance(&mut self, manual: bool) {
        let Some(current) = self.playing_track.or(self.selected()) else {
            return;
        };
        let next = if manual {
            self.forget_upcoming();
            self.choose_next(current, true)
        } else {
            self.upcoming_index(current)
        };
        if let Some(next) = next {
            dev::log(format!("next → #{next}"));
            self.request_play(next);
        } else {
            self.playing_track = None;
            self.was_playing = false;
            self.status_message = "Playlist finished.".to_string();
            dev::log("playlist finished");
            self.sync_system_media();
        }
    }

    fn play_previous(&mut self) {
        let Some(current) = self.playing_track.or(self.selected()) else {
            return;
        };
        // In shuffle, Previous walks back through what was actually played.
        if self.shuffle && self.history.len() >= 2 {
            self.history.pop(); // the current track
            if let Some(index) = self
                .history
                .pop()
                .and_then(|path| self.playlist.index_of(&path))
            {
                dev::log(format!("previous (shuffle history) → #{index}"));
                self.request_play(index);
                return;
            }
        }
        let len = self.playlist.tracks.len();
        let previous = if current > 0 {
            Some(current - 1)
        } else if self.repeat == RepeatMode::All && len > 0 {
            Some(len - 1)
        } else {
            None
        };
        if let Some(previous) = previous {
            dev::log(format!("previous → #{previous}"));
            self.request_play(previous);
        }
    }

    /// Pick the track after `current` under the repeat/shuffle rules.
    /// `manual` treats Repeat one like Repeat all (the user pressed Next).
    fn choose_next(&self, current: usize, manual: bool) -> Option<usize> {
        let tracks = &self.playlist.tracks;
        let played = |i: usize| self.history.contains(&tracks[i].path);
        let pool = next_candidates(current, tracks.len(), self.repeat, self.shuffle, manual, played);
        match pool.len() {
            0 => None,
            1 => Some(pool[0]),
            n => Some(pool[fastrand::usize(..n)]),
        }
    }

    /// The autoplay target, chosen once and remembered (see `upcoming`).
    fn upcoming_index(&mut self, current: usize) -> Option<usize> {
        if let Some(index) = self.upcoming.as_ref().and_then(|p| self.playlist.index_of(p)) {
            return Some(index);
        }
        let next = self.choose_next(current, false)?;
        self.upcoming = Some(self.playlist.tracks[next].path.clone());
        Some(next)
    }

    /// Drop the remembered upcoming track (and any gapless preload of it),
    /// e.g. after shuffle/repeat changes, a sort, or a removal.
    fn forget_upcoming(&mut self) {
        self.upcoming = None;
        if self.autoplay_preloaded {
            let _ = self.playback.clear_queued();
            self.autoplay_preloaded = false;
        }
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.history.clear();
        if self.shuffle
            && let Some(path) = self
                .playing_track
                .and_then(|i| self.playlist.tracks.get(i))
                .map(|t| t.path.clone())
        {
            self.history.push(path);
        }
        self.forget_upcoming();
        self.status_message = if self.shuffle { "Shuffle on." } else { "Shuffle off." }.to_string();
    }

    fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
        self.forget_upcoming();
        self.status_message = format!("{}.", self.repeat.label());
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        self.palette = self.theme.palette(system_dark(ctx));
        if self.applied_palette != Some(self.palette) {
            let theme = if self.palette.is_dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            };
            ctx.set_theme(theme);
            ctx.set_visuals_of(theme, self.palette.visuals());
            self.applied_palette = Some(self.palette);
        }
    }

    fn stop_playback(&mut self) {
        let _ = self.playback.pause();
        let _ = self.playback.seek_secs(0.0);
        self.seek_slider = 0.0;
        self.last_listen_at = None;
        self.status_message = "Stopped.".to_string();
        dev::log("stop");
        self.sync_system_media();
    }

    fn seek_relative(&mut self, delta_secs: f64) {
        let status = self.playback.status();
        let duration = status.duration_secs();
        if duration <= 0.0 {
            return;
        }
        let pos = (self.seek_slider + delta_secs).clamp(0.0, duration);
        self.seek_slider = pos;
        let _ = self.playback.seek_secs(pos);
        dev::log(format!("seek relative {delta_secs:+.0}s → {pos:.1}s"));
    }

    fn has_transport_target(&self) -> bool {
        !self.playlist.tracks.is_empty()
            && (self.playing_track.is_some()
                || self.selected().is_some()
                || self.playback.status().duration_secs() > 0.0)
    }

    fn can_play_or_pause(&self) -> bool {
        !self.playlist.tracks.is_empty()
    }

    fn cycle_sort_for_column(&mut self, column: SortColumn) {
        self.playlist_sort = match self.playlist_sort {
            Some((col, SortDirection::Asc)) if col == column => Some((column, SortDirection::Desc)),
            Some((col, SortDirection::Desc)) if col == column => None,
            _ => Some((column, SortDirection::Asc)),
        };
        self.apply_playlist_sort();
    }

    fn apply_playlist_sort(&mut self) {
        self.forget_upcoming();
        let playing_path = self
            .playing_track
            .and_then(|i| self.playlist.tracks.get(i).map(|t| t.path.clone()));
        // Selection is index-based; carry it across the reorder by path.
        let old_paths: Vec<PathBuf> = self.playlist.tracks.iter().map(|t| t.path.clone()).collect();

        match self.playlist_sort {
            Some((column, direction)) => self.playlist.sort_by(column, direction),
            None => self.playlist.restore_load_order(),
        }

        self.playing_track = playing_path
            .as_ref()
            .and_then(|path| self.playlist.index_of(path));
        let playlist = &self.playlist;
        self.selection
            .remap(|old| old_paths.get(old).and_then(|p| playlist.index_of(p)));
    }

    fn open_track_folder(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get(index) else {
            return;
        };
        match file_ops::open_enclosing_folder(&track.path) {
            Ok(()) => self.status_message = format!("Opened {}", track.filename_label()),
            Err(err) => self.status_message = err,
        }
    }

    /// Backspace/Delete: remove every selected row (or just the anchor).
    /// Removes highest index first so the remaining indices stay valid.
    fn remove_selected(&mut self) {
        let indices = if self.selection.is_empty() {
            self.selected().into_iter().collect()
        } else {
            self.selection.indices_desc()
        };
        let count = indices.len();
        if count == 0 {
            return;
        }
        let first = *indices.last().unwrap_or(&0);
        for index in indices {
            self.remove_from_playlist(index);
        }
        if count > 1 {
            self.status_message = format!("Removed {count} tracks from playlist");
            dev::log(format!("removed {count} tracks from playlist"));
        }
        // Land the focus where the removed block started.
        if !self.playlist.tracks.is_empty() {
            self.selection
                .select_only(first.min(self.playlist.tracks.len() - 1));
        }
    }

    fn remove_from_playlist(&mut self, index: usize) {
        let Some(removed) = self.playlist.remove_at(index) else {
            return;
        };
        if self.upcoming.as_ref() == Some(&removed.path) {
            self.forget_upcoming();
        }
        self.history.retain(|p| p != &removed.path);

        self.audio_cache.remove(&removed.path);
        self.waveform_cache.remove(&removed.path);

        if self.playing_track == Some(index) {
            let _ = self.playback.pause();
            self.playing_track = None;
            self.was_playing = false;
        } else if let Some(playing) = self.playing_track
            && playing > index
        {
            self.playing_track = Some(playing - 1);
        }

        self.selection.after_remove(index);
        if self.playlist.tracks.is_empty() {
            self.selection.clear();
        } else if self.selection.is_empty() {
            self.selection
                .select_only(index.min(self.playlist.tracks.len() - 1));
        }

        if self.confirm_delete == Some(index) {
            self.confirm_delete = None;
        } else if let Some(pending) = self.confirm_delete
            && pending > index
        {
            self.confirm_delete = Some(pending - 1);
        }

        self.status_message = format!("Removed {} from playlist", removed.display_label());
        dev::log(format!("removed from playlist: {}", removed.path.display()));
    }

    fn delete_from_disk(&mut self, index: usize) {
        let Some(path) = self.playlist.tracks.get(index).map(|t| t.path.clone()) else {
            return;
        };
        let label = self.playlist.tracks[index].display_label();

        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.remove_from_playlist(index);
                self.confirm_delete = None;
                self.status_message = format!("Deleted from disk: {label}");
                dev::log(format!("deleted from disk: {}", path.display()));
            }
            Err(err) => {
                self.confirm_delete = None;
                self.status_message = format!("Delete failed: {err}");
            }
        }
    }

    fn move_to_folder(&mut self, index: usize) {
        let Some(old_path) = self.playlist.tracks.get(index).map(|t| t.path.clone()) else {
            return;
        };
        let Some(file_name) = old_path.file_name().map(|n| n.to_owned()) else {
            self.status_message = "Move failed: invalid path".to_string();
            return;
        };

        let Some(folder) = rfd::FileDialog::new()
            .set_title("Move track to folder")
            .pick_folder()
        else {
            return;
        };

        let new_path = folder.join(&file_name);
        if new_path == old_path {
            self.status_message = "Already in that folder.".to_string();
            return;
        }
        if new_path.exists() {
            self.status_message = format!(
                "Move failed: {} already exists in destination",
                file_name.to_string_lossy()
            );
            return;
        }

        match file_ops::move_file(&old_path, &new_path) {
            Ok(()) => {
                if let Some(audio) = self.audio_cache.remove(&old_path) {
                    self.audio_cache.insert(new_path.clone(), audio);
                }
                if let Some(wave) = self.waveform_cache.remove(&old_path) {
                    self.waveform_cache.insert(new_path.clone(), wave);
                }
                if let Some(track) = self.playlist.tracks.get_mut(index) {
                    track.path = new_path.clone();
                }
                self.select_track(index);
                self.status_message = format!(
                    "Moved to {}",
                    folder.file_name().and_then(|s| s.to_str()).unwrap_or("folder")
                );
                dev::log(format!(
                    "moved {} → {}",
                    old_path.display(),
                    new_path.display()
                ));
            }
            Err(err) => {
                self.status_message = format!("Move failed: {err}");
            }
        }
    }

    fn write_tags_for(&mut self, index: usize) {
        let Some(track) = self.playlist.tracks.get(index) else {
            return;
        };
        let path = track.path.clone();
        let label = track.display_label();
        let (Some(bpm), Some(key)) = (track.bpm, track.key.clone()) else {
            self.status_message =
                "Write tags needs BPM and Key — wait for analysis to finish.".to_string();
            return;
        };

        match tags::write_analysis_tags(&path, bpm, &key) {
            Ok(true) => {
                self.status_message = format!("Wrote missing BPM/Key tags to {label}");
                dev::log(format!("wrote tags: {}", path.display()));
            }
            Ok(false) => {
                self.status_message =
                    format!("BPM/Key tags already present on {label} — left intact");
            }
            Err(err) => {
                self.status_message = format!("Tag write failed: {err}");
            }
        }
    }

    fn toggle_playback(&mut self) {
        let status = self.playback.status();
        if status.is_playing() {
            let _ = self.playback.pause();
            self.last_listen_at = None;
        } else if self.playing_track.is_some() {
            let _ = self.playback.play();
        } else if let Some(index) = self.selected() {
            self.request_play(index);
        }
        self.sync_system_media();
    }

    fn clear_loop(&mut self) {
        self.loop_beats = None;
        self.loop_start_secs = 0.0;
        self.loop_end_secs = 0.0;
        let _ = self.playback.clear_loop();
    }

    fn current_bpm(&self) -> Option<f32> {
        let index = self.playing_track.or(self.selected())?;
        self.playlist.tracks.get(index)?.bpm
    }

    fn set_beat_loop(&mut self, beats: u32) {
        if self.loop_beats == Some(beats) {
            self.clear_loop();
            self.status_message = "Loop off.".to_string();
            return;
        }

        let Some(bpm) = self.current_bpm().filter(|b| *b > 0.0) else {
            self.status_message = "Loop needs BPM — wait for analysis.".to_string();
            return;
        };

        let position = self.playback.status().position_secs();
        let beat_secs = 60.0 / bpm as f64;
        let start = (position / beat_secs).floor() * beat_secs;
        let end = start + beat_secs * beats as f64;

        let duration = self.playback.status().duration_secs();
        if duration > 0.0 && end > duration {
            self.status_message = "Loop extends past track end.".to_string();
            return;
        }

        self.loop_beats = Some(beats);
        self.loop_start_secs = start;
        self.loop_end_secs = end;
        let _ = self.playback.set_loop_secs(start, end);
        self.seek_slider = start.max(position);
        if position >= end {
            let _ = self.playback.seek_secs(start);
            self.seek_slider = start;
        }
        self.status_message = format!("{beats}-beat loop (quantized @ {bpm:.0} BPM)");
        dev::log(format!("{beats}-beat loop: {start:.3}s..{end:.3}s"));
    }

    /// Try to open the audio output again (after plugging in headphones etc.).
    fn retry_audio_output(&mut self) {
        match PlaybackEngine::new() {
            Ok(engine) => {
                self.playback = engine;
                let _ = self.playback.set_volume(self.volume);
                let _ = self.playback.set_eq(self.eq);
                self.playing_track = None;
                self.audio_error = None;
                self.status_message = "Audio output connected.".to_string();
                dev::log("audio output reconnected");
            }
            Err(err) => {
                self.audio_error = Some(err.to_string());
                self.status_message = format!("Still no audio output: {err}");
            }
        }
    }

    fn sync_system_media(&mut self) {
        let status = self.playback.status();
        let (title, artist) = self
            .playing_track
            .and_then(|i| self.playlist.tracks.get(i))
            .map(|t| (t.title.as_str(), t.artist.as_str()))
            .unwrap_or(("Qinip", ""));

        let duration = status.duration_secs();
        let Some(media) = self.system_media.as_mut() else {
            return;
        };
        media.set_metadata(title, artist, duration);
        media.set_playback(status.is_playing(), status.position_secs(), duration);
    }

    fn poll_system_media(&mut self) {
        let Some(media) = self.system_media.as_mut() else {
            return;
        };
        let events = media.poll();
        for event in events {
            match event {
                MediaControlEvent::Play => {
                    if !self.playback.status().is_playing() {
                        self.toggle_playback();
                    }
                }
                MediaControlEvent::Pause => {
                    if self.playback.status().is_playing() {
                        let _ = self.playback.pause();
                        self.last_listen_at = None;
                        self.sync_system_media();
                    }
                }
                MediaControlEvent::Toggle => self.toggle_playback(),
                MediaControlEvent::Next => self.play_next(),
                MediaControlEvent::Previous => self.play_previous(),
                MediaControlEvent::Stop => self.stop_playback(),
                MediaControlEvent::SetPosition(pos) => {
                    let secs = pos.0.as_secs_f64();
                    self.seek_slider = secs;
                    let _ = self.playback.seek_secs(secs);
                    self.sync_system_media();
                }
                _ => {}
            }
        }
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        // macOS delete-key often arrives as Backspace; Delete covers Windows/Linux.
        let remove = ctx.input(|i| {
            i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete)
        });
        // `command` is Cmd on macOS and Ctrl elsewhere.
        let select_all = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));

        if enter && let Some(index) = self.selected() {
            self.request_play(index);
        }
        if select_all {
            self.select_all_tracks();
        }
        if escape && self.selection.len() > 1 {
            // Collapse a multi-selection back to just the anchor.
            if let Some(anchor) = self.selected() {
                self.selection.select_only(anchor);
            }
        }
        if remove {
            self.remove_selected();
        }
    }

    fn accumulate_listen_time(&mut self, now: f64) {
        let playing = self.playback.status().is_playing();
        if !playing {
            self.last_listen_at = None;
            return;
        }

        let Some(index) = self.playing_track else {
            self.last_listen_at = None;
            return;
        };

        if let Some(last) = self.last_listen_at {
            let dt = now - last;
            if dt > 0.0 && dt < 1.0
                && let Some(track) = self.playlist.tracks.get_mut(index) {
                    track.mark_listened(dt);
                    if track.qualifies_as_played() {
                        self.was_playing = true;
                    }
                }
        }

        self.last_listen_at = Some(now);
    }

    fn maybe_autoplay_next(&mut self) -> bool {
        if self.playback.take_auto_advanced() {
            self.on_auto_advanced_to_next();
            return true;
        }

        let status = self.playback.status();
        let playing = status.is_playing();
        let duration = status.duration_secs();
        let position = status.position_secs();
        let near_end = duration > 0.5 && position >= duration - 0.35;

        if self.was_playing && self.autoplay && near_end && playing {
            self.preload_upcoming_track();
        }

        // Fallback when the next track wasn't preloaded (e.g. not decoded yet).
        let at_end = duration > 0.5 && position >= duration - 0.08;
        if self.was_playing && !playing && at_end && self.autoplay {
            self.was_playing = false;
            self.last_listen_at = None;
            self.autoplay_preloaded = false;
            self.advance(false);
            return true;
        }

        false
    }

    /// Stage the next playlist track in the playback engine before the current one ends.
    fn preload_upcoming_track(&mut self) {
        if self.autoplay_preloaded || !self.autoplay {
            return;
        }
        let Some(current) = self.playing_track else {
            return;
        };
        let Some(next) = self.upcoming_index(current) else {
            return;
        };
        let path = self.playlist.tracks[next].path.clone();
        let Some(audio) = self.audio_cache.get(&path).map(Arc::clone) else {
            return;
        };
        if self.playback.queue_next(audio).is_ok() {
            self.autoplay_preloaded = true;
            dev::log(format!("preloaded next track #{next} for gapless autoplay"));
        }
    }

    /// Audio thread already swapped to the queued track — update UI state to match.
    fn on_auto_advanced_to_next(&mut self) {
        // The audio thread swapped to whatever we preloaded: `upcoming`.
        let Some(next) = self
            .upcoming
            .take()
            .and_then(|path| self.playlist.index_of(&path))
        else {
            return;
        };
        if self.shuffle {
            self.history.push(self.playlist.tracks[next].path.clone());
        }

        self.playing_track = Some(next);
        self.selection.select_only(next);
        self.last_listen_at = None;
        self.autoplay_preloaded = false;
        self.clear_loop();
        self.was_playing = self
            .playlist
            .tracks
            .get(next)
            .is_some_and(|t| t.qualifies_as_played());
        self.status_message = format!(
            "Playing {}",
            self.playlist.tracks[next].display_label()
        );
        dev::log(format!("gapless autoplay → #{next}"));
        self.sync_system_media();
    }

    fn seek_from_waveform_click(&mut self, fraction: f64) {
        let duration = self.playback.status().duration_secs();
        if duration <= 0.0 {
            return;
        }
        let secs = (fraction * duration).clamp(0.0, duration);
        self.seek_slider = secs;
        let _ = self.playback.seek_secs(secs);
        dev::log(format!("waveform seek -> {secs:.1}s"));
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let mut folders = Vec::new();
        let mut files = Vec::new();

        ctx.input(|input| {
            for dropped in &input.raw.dropped_files {
                let path = dropped.path();
                if path.is_dir() {
                    folders.push(path.to_path_buf());
                } else {
                    files.push(path.to_path_buf());
                }
            }
        });

        // Every dropped folder becomes its own group; loose files come after.
        for folder in folders {
            self.load_folder(folder);
        }
        if !files.is_empty() {
            self.load_paths(files);
        }
    }

    fn draw_eq_knobs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("EQ");
            ui.add_space(8.0);

            let mut changed = false;
            let mut gains = [
                (&mut self.eq.low, BAND_LABELS[0], BAND_COLORS[0]),
                (&mut self.eq.low_mid, BAND_LABELS[1], BAND_COLORS[1]),
                (&mut self.eq.hi_mid, BAND_LABELS[2], BAND_COLORS[2]),
                (&mut self.eq.hi, BAND_LABELS[3], BAND_COLORS[3]),
            ];

            for (gain, label, color) in &mut gains {
                let (r, g, b) = *color;
                let knob_color = egui::Color32::from_rgb(r, g, b);

                ui.allocate_ui(egui::vec2(58.0, 88.0), |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(*label)
                                .color(knob_color)
                                .small(),
                        );
                        if eq_knob(ui, gain, knob_color).changed() {
                            changed = true;
                        }
                        ui.label(format!("{:.0}%", **gain * 100.0));
                    });
                });
            }

            if ui.button("Reset EQ").clicked() {
                self.eq = EqGains::default();
                changed = true;
            }

            if changed {
                let _ = self.playback.set_eq(self.eq);
            }

            ui.separator();
            self.draw_transport_controls(ui);
        });
    }

    fn draw_transport_controls(&mut self, ui: &mut egui::Ui) {
        let playing = self.playback.status().is_playing();
        let can_transport = self.has_transport_target();
        let can_play = self.can_play_or_pause();
        let can_loop = self.current_bpm().is_some();

        let green = egui::Color32::from_rgb(45, 198, 83);
        let amber = egui::Color32::from_rgb(232, 150, 4);
        let cyan = egui::Color32::from_rgb(76, 201, 240);
        let magenta = egui::Color32::from_rgb(220, 80, 200);

        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Transport").small().weak());
            ui.horizontal(|ui| {
                if transport_button(ui, "⏮", cyan, can_transport, false) {
                    self.play_previous();
                }
                if transport_button(
                    ui,
                    if playing { "⏸" } else { "▶" },
                    green,
                    can_play,
                    playing,
                ) {
                    self.toggle_playback();
                }
                if transport_button(ui, "⏭", cyan, can_transport, false) {
                    self.play_next();
                }
                if transport_button(ui, "⏹", amber, can_transport, false) {
                    self.stop_playback();
                }
                if transport_button(ui, "−10", cyan, can_transport, false) {
                    self.seek_relative(-10.0);
                }
                if transport_button(ui, "+10", cyan, can_transport, false) {
                    self.seek_relative(10.0);
                }
                if transport_button(ui, "🔀", green, can_play, self.shuffle) {
                    self.toggle_shuffle();
                }
                let repeat_icon = if self.repeat == RepeatMode::One { "🔂" } else { "🔁" };
                if transport_button(
                    ui,
                    repeat_icon,
                    green,
                    can_play,
                    self.repeat != RepeatMode::Off,
                ) {
                    self.cycle_repeat();
                }

                ui.separator();

                for beats in [2u32, 4, 8, 16] {
                    let active = self.loop_beats == Some(beats);
                    if transport_button(
                        ui,
                        &beats.to_string(),
                        magenta,
                        can_loop,
                        active,
                    ) {
                        self.set_beat_loop(beats);
                    }
                }

                ui.separator();

                let position = self.playback.status().position_secs();
                beat_bar_indicator(ui, position, self.current_bpm(), &self.palette);

                ui.separator();
                self.draw_volume(ui);
            });
        });
    }

    /// Speaker button (mute toggle) + volume fader with a percentage readout.
    fn draw_volume(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Volume").small().weak());
            ui.horizontal(|ui| {
                let muted = self.volume <= 0.001;
                let icon = if muted { "🔇" } else { "🔊" };
                let mut changed = false;
                if ui
                    .button(icon)
                    .on_hover_text(if muted { "Unmute" } else { "Mute" })
                    .clicked()
                {
                    if muted {
                        self.volume = if self.volume_before_mute > 0.001 {
                            self.volume_before_mute
                        } else {
                            0.85
                        };
                    } else {
                        self.volume_before_mute = self.volume;
                        self.volume = 0.0;
                    }
                    changed = true;
                }

                ui.spacing_mut().slider_width = 120.0;
                if ui
                    .add(
                        egui::Slider::new(&mut self.volume, 0.0..=1.0)
                            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                            .trailing_fill(true),
                    )
                    .changed()
                {
                    changed = true;
                }

                if changed {
                    let _ = self.playback.set_volume(self.volume);
                }
            });
        });
    }

    fn draw_waveform(&mut self, ui: &mut egui::Ui) {
        let index = match self.playing_track.or(self.selected()) {
            Some(i) => i,
            None => return,
        };

        let path = match self.playlist.tracks.get(index) {
            Some(t) => t.path.clone(),
            None => return,
        };

        let Some(bands) = self.waveform_cache.get(&path).cloned() else {
            ui.label("Waveform will appear once the track is decoded…");
            return;
        };

        let label = self.playlist.tracks[index].display_label();
        ui.label(egui::RichText::new(label).strong());

        self.draw_eq_knobs(ui);

        let desired = egui::vec2(ui.available_width(), 110.0);
        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::CLICK);
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, 4.0, self.palette.screen_bg);

        let layers = [
            (&bands.low, self.eq.low, BAND_COLORS[0]),
            (&bands.low_mid, self.eq.low_mid, BAND_COLORS[1]),
            (&bands.hi_mid, self.eq.hi_mid, BAND_COLORS[2]),
            (&bands.hi, self.eq.hi, BAND_COLORS[3]),
        ];

        let mid_y = rect.center().y;
        let half_h = rect.height() * 0.45;
        let width = rect.width();
        let n = bands
            .low
            .len()
            .max(bands.low_mid.len())
            .max(bands.hi_mid.len())
            .max(bands.hi.len())
            .max(1);

        for (peaks, gain, (r, g, b)) in layers {
            if gain <= 0.001 || peaks.is_empty() {
                continue;
            }

            let alpha = ((gain / 2.0) * 220.0).clamp(20.0, 220.0) as u8;
            let color = egui::Color32::from_rgba_unmultiplied(r, g, b, alpha);

            for (i, &(min, max)) in peaks.iter().enumerate() {
                let x = rect.left() + (i as f32 / n as f32) * width;
                let y_top = mid_y - (max * gain).clamp(0.0, 1.5).min(1.0) * half_h;
                let y_bot = mid_y - (min * gain).clamp(-1.5, 0.0).max(-1.0) * half_h;
                painter.line_segment(
                    [egui::pos2(x, y_top), egui::pos2(x, y_bot)],
                    egui::Stroke::new(1.0, color),
                );
            }
        }

        let duration = self.playback.status().duration_secs();
        if duration > 0.0 {
            let fraction = (self.seek_slider / duration).clamp(0.0, 1.0) as f32;
            let x = rect.left() + fraction * width;
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(2.0, self.palette.playhead),
            );

            if self.loop_beats.is_some() {
                let start_frac =
                    (self.loop_start_secs / duration).clamp(0.0, 1.0) as f32;
                let end_frac = (self.loop_end_secs / duration).clamp(0.0, 1.0) as f32;
                let loop_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + start_frac * width, rect.top()),
                    egui::pos2(rect.left() + end_frac * width, rect.bottom()),
                );
                painter.rect_filled(
                    loop_rect,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(220, 80, 200, 40),
                );
                painter.rect_stroke(
                    loop_rect,
                    0.0,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 80, 200)),
                    egui::StrokeKind::Inside,
                );
            }
        }

        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos() {
                let fraction = ((pos.x - rect.left()) / width).clamp(0.0, 1.0) as f64;
                self.seek_from_waveform_click(fraction);
            }
    }

    fn draw_title_cell(&self, ui: &mut egui::Ui, index: usize, width: f32) -> egui::Response {
        let track = &self.playlist.tracks[index];
        let selected = self.selection.contains(index);
        let is_anchor = self.selected() == Some(index);
        let playing = self.playing_track == Some(index);
        let label = if playing {
            format!("▶ {}", track.title)
        } else {
            track.title.clone()
        };

        let fill = match &track.analysis {
            AnalysisState::Failed(_) => self.palette.fill_failed,
            AnalysisState::Done => self.palette.fill_done,
            AnalysisState::Loading | AnalysisState::Analyzing => self.palette.fill_busy,
            AnalysisState::Pending => self.palette.fill_pending,
        };

        let progress = track.analysis_progress();
        let animate = matches!(
            track.analysis,
            AnalysisState::Loading | AnalysisState::Analyzing
        );

        let desired = egui::vec2(width, 22.0);
        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::CLICK);
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, self.palette.row_bg);
        let filled = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * progress, rect.height()),
        );
        painter.rect_filled(filled, 3.0, fill);

        if selected {
            // Members of a multi-selection get an amber wash; the anchor row
            // keeps the crisp white outline so the focused track stays obvious.
            let accent = self.palette.accent;
            painter.rect_filled(rect, 3.0, accent.gamma_multiply(0.18));
            let (stroke_w, stroke_color) = if is_anchor {
                (1.5, self.palette.focus_outline)
            } else {
                (1.0, accent)
            };
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(stroke_w, stroke_color),
                egui::StrokeKind::Inside,
            );
        }

        if animate {
            let pulse = ((ui.input(|i| i.time) as f32 * 3.0).sin() * 0.5 + 0.5) * 40.0;
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, self.palette.text_weak.gamma_multiply(0.7 + pulse / 133.0)),
                egui::StrokeKind::Outside,
            );
        }

        painter.text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            self.palette.row_text,
        );

        response
            .on_hover_text(track.filename_label())
    }
}

impl eframe::App for QinipApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            SETTINGS_KEY,
            &Settings {
                volume: self.volume,
                autoplay: self.autoplay,
                auto_write_tags: self.auto_write_tags,
                theme: self.theme,
                repeat: self.repeat,
                shuffle: self.shuffle,
            },
        );
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_theme(ctx);
        self.playback.poll();
        self.poll_worker();
        self.poll_system_media();
        self.handle_drops(ctx);
        self.handle_keyboard(ctx);

        let now = ctx.input(|i| i.time);
        self.accumulate_listen_time(now);
        if self.maybe_autoplay_next() {
            // Apply Load/Play (or detect gapless swap) in the same frame — avoids a
            // visible pause between autoplay commands and the audio thread.
            self.playback.poll();
            ctx.request_repaint();
        }

        let status = self.playback.status();
        if status.is_playing() {
            self.seek_slider = status.position_secs();
            self.sync_system_media();
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if self.autoplay && self.playing_track.is_some() {
            // Keep polling near track boundaries so gapless preload/swap is timely.
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        } else if self.playlist.tracks.iter().any(|t| {
            matches!(
                t.analysis,
                AnalysisState::Loading | AnalysisState::Analyzing
            )
        }) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let status = self.playback.status();

        if let Some(index) = self.confirm_delete {
            let label = self
                .playlist
                .tracks
                .get(index)
                .map(|t| t.display_label())
                .unwrap_or_else(|| "this track".to_string());
            let path_display = self
                .playlist
                .tracks
                .get(index)
                .map(|t| t.path.display().to_string())
                .unwrap_or_default();

            let mut open = true;
            egui::Window::new("Delete from disk?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Permanently delete:\n{label}"));
                    ui.label(
                        egui::RichText::new(&path_display)
                            .small()
                            .weak(),
                    );
                    ui.label(
                        egui::RichText::new("This cannot be undone.")
                            .color(egui::Color32::LIGHT_RED),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.confirm_delete = None;
                        }
                        if ui
                            .button(egui::RichText::new("Delete file").color(egui::Color32::RED))
                            .clicked()
                        {
                            self.delete_from_disk(index);
                        }
                    });
                });
            if !open {
                self.confirm_delete = None;
            }
        }

        if self.show_about {
            let mut open = true;
            egui::Window::new("About Qinip")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.heading(format!("Qinip {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Free MP3 pack audition player.");
                    ui.add_space(6.0);
                    ui.label(
                        "Qinip is free software under the GNU General Public License v3 \
                         or later. You may use, share and change it. Source code:",
                    );
                    ui.hyperlink(env!("CARGO_PKG_REPOSITORY"));
                    ui.add_space(6.0);
                    ui.label(
                        "BPM and key detection by aubio (GPL-3.0, aubio.org). Built with \
                         egui, cpal, symphonia, souvlaki and other open-source crates. \
                         Their licenses are in THIRD_PARTY_LICENSES.html, inside the \
                         app's Resources folder and in the source repository.",
                    );
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Qinip makes no network connections.").weak());
                });
            if !open {
                self.show_about = false;
            }
        }

        if let Some(err) = self.audio_error.clone() {
            egui::Panel::top("audio_banner").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "No audio output ({err}). Connect speakers or headphones, then retry."
                        ))
                        .color(self.palette.accent_text),
                    );
                    if ui.button("Retry audio").clicked() {
                        self.retry_audio_output();
                    }
                });
            });
        }

        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Load Folder").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    self.load_folder(folder);
                }

                ui.checkbox(&mut self.autoplay, "Autoplay");
                ui.checkbox(&mut self.auto_write_tags, "Write BPM/Key to files")
                    .on_hover_text(
                        "When on, Qinip writes the analyzed BPM and key into each MP3's \
                         ID3 tags (only fills missing tags, never overwrites). Off by \
                         default: your files are not changed unless you turn this on or \
                         use right-click → Write BPM/Key tags.",
                    );
                egui::ComboBox::from_id_salt("theme_picker")
                    .selected_text(self.theme.label())
                    .show_ui(ui, |ui| {
                        for choice in ThemeChoice::ALL {
                            ui.selectable_value(&mut self.theme, choice, choice.label());
                        }
                    })
                    .response
                    .on_hover_text("Colour theme");
                if ui.button("About").clicked() {
                    self.show_about = true;
                }

                ui.label(&self.status_message);

                if dev::enabled() {
                    ui.label(
                        egui::RichText::new("DEV")
                            .color(egui::Color32::YELLOW)
                            .strong(),
                    );
                }
            });
        });

        egui::Panel::bottom("transport").show(ui, |ui| {
            ui.horizontal(|ui| {
                let duration = status.duration_secs();
                ui.label(format!(
                    "{} / {}",
                    format_clock(status.position_secs()),
                    format_clock(duration)
                ));

                let mut seek = self.seek_slider;
                let response = ui.add(
                    egui::Slider::new(&mut seek, 0.0..=duration.max(0.001))
                        .show_value(false)
                        .trailing_fill(true),
                );
                if response.dragged() || response.clicked() {
                    self.seek_slider = seek;
                    let _ = self.playback.seek_secs(seek);
                }

            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            self.draw_waveform(ui);
            ui.separator();

            ui.heading("Playlist");
            ui.label(
                egui::RichText::new(
                    "Click = select · Double-click / Enter = play · Header click = sort (▲/▼/off) · Drag header edge = resize column · Right-click = more",
                )
                .small()
                .weak(),
            );
            ui.separator();

            if self.playlist.tracks.is_empty() {
                ui.label("Drop a folder of MP3s here, or use Load Folder.");
                return;
            }

            let mut pending_select = None;
            let mut pending_play = None;
            let mut pending_remove = None;
            let mut pending_delete = None;
            let mut pending_move = None;
            let mut pending_tags = None;
            let mut pending_open_folder = None;
            let mut pending_sort: Option<SortColumn> = None;

            let avail = ui.available_width();
            let spacing = 12.0;
            let sort_state = self.playlist_sort;
            let mut cols = self.playlist_columns.clone();
            let title_w = cols.title_width(avail, spacing);

            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(avail);
                    egui::Grid::new("playlist_grid")
                        .num_columns(8)
                        .spacing([spacing, 6.0])
                        .striped(true)
                        .min_col_width(36.0)
                        .show(ui, |ui| {
                            let track_hdr = draw_column_header(
                                ui,
                                "#",
                                cols.track_num,
                                SortColumn::TrackNumber,
                                sort_state,
                            );
                            if track_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::TrackNumber);
                            }
                            cols.track_num = track_hdr.width;

                            let artist_hdr = draw_column_header(
                                ui,
                                "Artist",
                                cols.artist,
                                SortColumn::Artist,
                                sort_state,
                            );
                            if artist_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::Artist);
                            }
                            cols.artist = artist_hdr.width;

                            let title_hdr = draw_column_header(
                                ui,
                                "Title",
                                title_w,
                                SortColumn::Title,
                                sort_state,
                            );
                            if title_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::Title);
                            }
                            cols.title = title_hdr.width.max(120.0);
                            let title_w = cols.title_width(avail, spacing).max(cols.title);

                            let dur_hdr = draw_column_header(
                                ui,
                                "Duration",
                                cols.duration,
                                SortColumn::Duration,
                                sort_state,
                            );
                            if dur_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::Duration);
                            }
                            cols.duration = dur_hdr.width;

                            let bpm_hdr = draw_column_header(
                                ui,
                                "BPM",
                                cols.bpm,
                                SortColumn::Bpm,
                                sort_state,
                            );
                            if bpm_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::Bpm);
                            }
                            cols.bpm = bpm_hdr.width;

                            let key_hdr = draw_column_header(
                                ui,
                                "Key",
                                cols.key,
                                SortColumn::Key,
                                sort_state,
                            );
                            if key_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::Key);
                            }
                            cols.key = key_hdr.width;

                            let cam_hdr = draw_column_header(
                                ui,
                                "Camelot",
                                cols.camelot,
                                SortColumn::Camelot,
                                sort_state,
                            );
                            if cam_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::Camelot);
                            }
                            cols.camelot = cam_hdr.width;

                            let played_hdr = draw_column_header(
                                ui,
                                "Played",
                                cols.played,
                                SortColumn::Played,
                                sort_state,
                            );
                            if played_hdr.sort_clicked {
                                pending_sort = Some(SortColumn::Played);
                            }
                            cols.played = played_hdr.width;

                            ui.end_row();

                            // Folder separators: only meaningful in load-order view
                            // (sorting interleaves folders), and only worth the noise
                            // when more than one source is loaded.
                            let show_groups =
                                sort_state.is_none() && self.playlist.distinct_source_count() > 1;
                            let mut prev_source: Option<Option<&Path>> = None;

                            for index in 0..self.playlist.tracks.len() {
                                let track = &self.playlist.tracks[index];

                                if show_groups {
                                    let source = track.source_folder.as_deref();
                                    if prev_source != Some(source) {
                                        let count = self
                                            .playlist
                                            .tracks
                                            .iter()
                                            .filter(|t| t.source_folder.as_deref() == source)
                                            .count();
                                        draw_folder_header(ui, source, count);
                                        ui.end_row();
                                        prev_source = Some(source);
                                    }
                                }

                                playlist_cell_label(ui, track.track_number_label(), cols.track_num);
                                playlist_cell_label(ui, &track.artist, cols.artist);

                                let response = self.draw_title_cell(ui, index, title_w);
                                if response.double_clicked() {
                                    pending_play = Some(index);
                                } else if response.clicked() {
                                    let modifiers = ui.input(|i| i.modifiers);
                                    pending_select = Some((index, modifiers));
                                }
                                response.context_menu(|ui| {
                                    if ui.button("Play").clicked() {
                                        pending_play = Some(index);
                                        ui.close();
                                    }
                                    if ui.button("Remove from playlist").clicked() {
                                        pending_remove = Some(index);
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button("Open enclosing folder").clicked() {
                                        pending_open_folder = Some(index);
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui.button("Move to folder…").clicked() {
                                        pending_move = Some(index);
                                        ui.close();
                                    }
                                    if ui.button("Write BPM/Key tags").clicked() {
                                        pending_tags = Some(index);
                                        ui.close();
                                    }
                                    ui.separator();
                                    if ui
                                        .button(
                                            egui::RichText::new("Delete from disk…")
                                                .color(egui::Color32::LIGHT_RED),
                                        )
                                        .clicked()
                                    {
                                        pending_delete = Some(index);
                                        ui.close();
                                    }
                                });

                                let track = &self.playlist.tracks[index];
                                playlist_cell_label(ui, track.duration_label(), cols.duration);

                                let value_color = match &track.analysis {
                                    AnalysisState::Failed(_) => self.palette.value_failed,
                                    AnalysisState::Done => self.palette.value_done,
                                    AnalysisState::Loading | AnalysisState::Analyzing => {
                                        self.palette.value_busy
                                    }
                                    AnalysisState::Pending => self.palette.value_pending,
                                };

                                playlist_cell_rich(
                                    ui,
                                    track.bpm_label(),
                                    cols.bpm,
                                    value_color,
                                );
                                playlist_cell_rich(
                                    ui,
                                    track.key_label(),
                                    cols.key,
                                    value_color,
                                );
                                playlist_cell_rich(
                                    ui,
                                    track.camelot_label(),
                                    cols.camelot,
                                    value_color,
                                );
                                playlist_cell_label(
                                    ui,
                                    if track.played { "✓" } else { "—" },
                                    cols.played,
                                );
                                ui.end_row();
                            }
                        });
                });

            self.playlist_columns = cols;

            if let Some(column) = pending_sort {
                self.cycle_sort_for_column(column);
            }
            if let Some(index) = pending_play {
                self.request_play(index);
            } else if let Some((index, modifiers)) = pending_select {
                self.click_track(index, modifiers);
            }
            if let Some(index) = pending_remove {
                self.remove_from_playlist(index);
            }
            if let Some(index) = pending_delete {
                self.select_track(index);
                self.confirm_delete = Some(index);
            }
            if let Some(index) = pending_move {
                self.move_to_folder(index);
            }
            if let Some(index) = pending_tags {
                self.write_tags_for(index);
            }
            if let Some(index) = pending_open_folder {
                self.open_track_folder(index);
            }
        });
    }
}

struct ColumnHeaderResult {
    sort_clicked: bool,
    width: f32,
}

/// A group header row for one source folder (or the loose-files group).
/// egui's Grid has no colspan, so the label sits in the first cell and the
/// caller ends the row; the amber rule underneath reads as a separator.
fn draw_folder_header(ui: &mut egui::Ui, source: Option<&Path>, count: usize) {
    let amber = ui.visuals().warn_fg_color;
    let name = match source {
        Some(folder) => Playlist::source_folder_label(folder),
        None => "Dropped files".to_string(),
    };
    let text = format!("📁 {name}  ·  {count}");
    let full_path = source.map(|p| p.display().to_string()).unwrap_or_default();

    let resp = ui.add(
        egui::Label::new(egui::RichText::new(text).color(amber).strong())
            .selectable(false)
            .wrap_mode(egui::TextWrapMode::Extend),
    );
    if !full_path.is_empty() {
        resp.on_hover_text(full_path);
    }
}

/// Click the label to cycle sort (asc → desc → none). Drag the right edge to resize.
fn draw_column_header(
    ui: &mut egui::Ui,
    label: &str,
    width: f32,
    column: SortColumn,
    active_sort: Option<(SortColumn, SortDirection)>,
) -> ColumnHeaderResult {
    let total_w = width + 6.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total_w, 20.0), egui::Sense::hover());
    let label_rect = egui::Rect::from_min_size(rect.min, egui::vec2(width, rect.height()));
    let resize_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + width, rect.top()),
        egui::vec2(6.0, rect.height()),
    );

    let text = sort_header_label(label, column, active_sort);
    let label_resp = ui.interact(label_rect, ui.id().with(label).with(column), egui::Sense::click());
    let sort_clicked = label_resp.clicked();
    let _ = label_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let is_sorted = active_sort.is_some_and(|(col, _)| col == column);
    let header_color = if is_sorted {
        ui.visuals().warn_fg_color
    } else {
        ui.visuals().text_color()
    };
    ui.painter().text(
        label_rect.left_center() + egui::vec2(0.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(13.0),
        header_color,
    );

    let resize_resp = ui.interact(resize_rect, ui.id().with(label).with("resize"), egui::Sense::drag());
    let resize_delta = resize_resp.drag_delta();
    let _ = resize_resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    ui.painter().line_segment(
        [
            egui::pos2(resize_rect.center().x, resize_rect.top() + 2.0),
            egui::pos2(resize_rect.center().x, resize_rect.bottom() - 2.0),
        ],
        egui::Stroke::new(1.0, ui.visuals().weak_text_color()),
    );

    let mut new_width = width;
    if resize_delta.x != 0.0 {
        new_width = (width + resize_delta.x).clamp(36.0, 600.0);
    }

    ColumnHeaderResult {
        sort_clicked,
        width: new_width,
    }
}

fn sort_header_label(
    label: &str,
    column: SortColumn,
    active_sort: Option<(SortColumn, SortDirection)>,
) -> String {
    match active_sort {
        Some((col, SortDirection::Asc)) if col == column => format!("{label} ▲"),
        Some((col, SortDirection::Desc)) if col == column => format!("{label} ▼"),
        _ => label.to_string(),
    }
}

fn playlist_cell_label(ui: &mut egui::Ui, text: impl AsRef<str>, width: f32) {
    let resp = ui.add(
        egui::Label::new(text.as_ref())
            .selectable(false)
            .truncate()
            .sense(egui::Sense::click()),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let _ = width;
}

fn playlist_cell_rich(ui: &mut egui::Ui, text: String, width: f32, color: egui::Color32) {
    let resp = ui.add(
        egui::Label::new(egui::RichText::new(text).color(color))
            .selectable(false)
            .sense(egui::Sense::click()),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let _ = width;
}

/// Load the app window icon from the bundled PNG/JPEG asset.
pub fn load_app_icon() -> egui::IconData {
    let bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory(bytes).expect("app icon must decode");
    let img = img.to_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    }
}

/// Which tracks may come after `current`. Empty = stop. Sequential play
/// yields exactly one; shuffle yields every eligible track (caller picks one
/// at random). `played(i)` = track `i` already had its turn this shuffle cycle.
fn next_candidates(
    current: usize,
    len: usize,
    repeat: RepeatMode,
    shuffle: bool,
    manual: bool,
    played: impl Fn(usize) -> bool,
) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    if repeat == RepeatMode::One && !manual {
        return vec![current];
    }
    let wrap = repeat != RepeatMode::Off;

    if shuffle {
        if len == 1 {
            return if wrap { vec![current] } else { Vec::new() };
        }
        let unplayed: Vec<usize> = (0..len).filter(|&i| i != current && !played(i)).collect();
        if !unplayed.is_empty() {
            return unplayed;
        }
        // Everyone had a turn: a fresh cycle if wrapping, else stop.
        return if wrap {
            (0..len).filter(|&i| i != current).collect()
        } else {
            Vec::new()
        };
    }

    if current + 1 < len {
        vec![current + 1]
    } else if wrap {
        vec![0]
    } else {
        Vec::new()
    }
}

/// The OS light/dark preference, if the platform reports one.
fn system_dark(ctx: &egui::Context) -> Option<bool> {
    ctx.system_theme().map(|theme| theme == egui::Theme::Dark)
}

/// Runs the actual BPM + key detection, catching any panic so a single
/// malformed/unusual track can never kill the analysis thread out from
/// under the app (mirrors the `catch_unwind` guard already used for decode
/// in `worker.rs`).
fn run_bpm_key_analysis(audio: &DecodedAudio) -> Result<(f32, String), String> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let bpm_result = detect_bpm(audio);
        let key_result = detect_key(audio);
        (bpm_result.bpm, key_result.key_name)
    }));

    result.map_err(|_| "BPM/key analysis panicked".to_string())
}

fn format_clock(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let minutes = total / 60;
    let seconds = total % 60;
    format!("{minutes}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE_PLAYED: fn(usize) -> bool = |_| false;

    #[test]
    fn sequential_stops_at_end_unless_repeat_all() {
        assert_eq!(next_candidates(1, 3, RepeatMode::Off, false, false, NONE_PLAYED), vec![2]);
        assert!(next_candidates(2, 3, RepeatMode::Off, false, false, NONE_PLAYED).is_empty());
        assert_eq!(next_candidates(2, 3, RepeatMode::All, false, false, NONE_PLAYED), vec![0]);
    }

    #[test]
    fn repeat_one_replays_on_autoplay_but_next_moves_on() {
        assert_eq!(next_candidates(1, 3, RepeatMode::One, false, false, NONE_PLAYED), vec![1]);
        assert_eq!(next_candidates(1, 3, RepeatMode::One, false, true, NONE_PLAYED), vec![2]);
        // Manual Next on the last track with Repeat one wraps like Repeat all.
        assert_eq!(next_candidates(2, 3, RepeatMode::One, false, true, NONE_PLAYED), vec![0]);
    }

    #[test]
    fn shuffle_skips_current_and_already_played() {
        let played = |i: usize| i == 0 || i == 3;
        assert_eq!(next_candidates(1, 5, RepeatMode::Off, true, false, played), vec![2, 4]);
    }

    #[test]
    fn shuffle_cycle_ends_or_restarts() {
        let all_played = |_: usize| true;
        assert!(next_candidates(1, 4, RepeatMode::Off, true, false, all_played).is_empty());
        assert_eq!(next_candidates(1, 4, RepeatMode::All, true, false, all_played), vec![0, 2, 3]);
    }
}
