use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui;

use crate::bpm::detect_bpm;
use crate::decode::DecodedAudio;
use crate::dev;
use crate::eq::{band_waveform_peaks, BandWaveform, EqGains, BAND_COLORS, BAND_LABELS};
use crate::key::detect_key;
use crate::knob::eq_knob;
use crate::playback::PlaybackEngine;
use crate::playlist::{AnalysisState, Playlist};
use crate::worker::{AnalysisWorker, WorkerJob, WorkerUpdate};

struct AnalysisResult {
    path: PathBuf,
    bpm: f32,
    key: String,
}

pub struct DjApp {
    playlist: Playlist,
    playback: PlaybackEngine,
    worker: AnalysisWorker,
    updates_rx: Receiver<WorkerUpdate>,
    analysis_tx: Sender<AnalysisResult>,
    analysis_rx: Receiver<AnalysisResult>,
    audio_cache: HashMap<PathBuf, Arc<DecodedAudio>>,
    waveform_cache: HashMap<PathBuf, BandWaveform>,
    selected: Option<usize>,
    playing_track: Option<usize>,
    volume: f32,
    eq: EqGains,
    seek_slider: f64,
    status_message: String,
    autoplay: bool,
    was_playing: bool,
    last_listen_at: Option<f64>,
}

impl DjApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        if dev::enabled() {
            eprintln!("[dev] DJ_PLAYER_DEV enabled — logging to stderr");
            eprintln!("[dev] tip: use `cargo run --release` for ~18x faster analysis");
        }

        let playback = PlaybackEngine::new().expect("failed to start audio output");
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
            selected: None,
            playing_track: None,
            volume: 0.85,
            eq: EqGains::default(),
            seek_slider: 0.0,
            status_message: "Load a folder or drop MP3s to begin.".to_string(),
            autoplay: true,
            was_playing: false,
            last_listen_at: None,
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
            if let Some(track_index) = self.playlist.index_of(&result.path)
                && let Some(track) = self.playlist.tracks.get_mut(track_index)
            {
                track.set_analysis_result(result.bpm, result.key.clone());
                dev::log(format!(
                    "analysis done {} — {:.1} BPM, {} ({})",
                    result.path.display(),
                    result.bpm,
                    result.key,
                    track.camelot.as_deref().unwrap_or("?")
                ));
            }
        }
    }

    fn spawn_analysis(&self, path: PathBuf, audio: Arc<DecodedAudio>) {
        let tx = self.analysis_tx.clone();
        thread::spawn(move || {
            let start = std::time::Instant::now();
            let bpm_result = detect_bpm(&audio);
            let key_result = detect_key(&audio);
            dev::log_timing(&format!("bpm+key {}", path.display()), start);

            let _ = tx.send(AnalysisResult {
                path,
                bpm: bpm_result.bpm,
                key: key_result.key_name,
            });
        });
    }

    fn load_folder(&mut self, folder: PathBuf) {
        let added = self.playlist.append_folder(&folder);
        self.status_message = format!(
            "Added {added} tracks from {} — analyzing…",
            folder.display()
        );
        dev::log(format!("folder appended ({added}): {}", folder.display()));
        self.analyze_pending();
    }

    fn load_paths(&mut self, paths: Vec<PathBuf>) {
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

    fn select_track(&mut self, index: usize) {
        self.selected = Some(index);
        if let Some(track) = self.playlist.tracks.get(index) {
            self.status_message = format!("Selected {}", track.display_label());
            dev::log(format!("selected track #{index}"));
        }
    }

    fn request_play(&mut self, index: usize) {
        self.selected = Some(index);

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
        let Some(audio) = self.audio_cache.get(&path).map(|a| (**a).clone()) else {
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
        self.selected = Some(index);
        self.last_listen_at = None;
        self.was_playing = self
            .playlist
            .tracks
            .get(index)
            .is_some_and(|t| t.qualifies_as_played());
        self.status_message = format!("Playing {}", self.playlist.tracks[index].display_label());
        dev::log(format!("playing track #{index}"));
    }

    fn play_next(&mut self) {
        let Some(current) = self.playing_track else {
            return;
        };
        let next = current + 1;
        if next < self.playlist.tracks.len() {
            dev::log(format!("autoplay next → #{next}"));
            self.request_play(next);
        } else {
            self.playing_track = None;
            self.was_playing = false;
            self.status_message = "Playlist finished.".to_string();
            dev::log("playlist finished");
        }
    }

    fn remove_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };

        let Some(removed) = self.playlist.remove_at(index) else {
            return;
        };

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

        if self.playlist.tracks.is_empty() {
            self.selected = None;
        } else {
            self.selected = Some(index.min(self.playlist.tracks.len() - 1));
        }

        self.status_message = format!("Removed {} from playlist", removed.display_label());
        dev::log(format!("removed from playlist: {}", removed.path.display()));
    }

    fn toggle_playback(&mut self) {
        let status = self.playback.status();
        if status.is_playing() {
            let _ = self.playback.pause();
            self.last_listen_at = None;
        } else if self.playing_track.is_some() {
            let _ = self.playback.play();
        } else if let Some(index) = self.selected {
            self.request_play(index);
        }
    }

    fn handle_keyboard(&mut self, ctx: &egui::Context) {
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
        // macOS delete-key often arrives as Backspace; Delete covers Windows/Linux.
        let remove = ctx.input(|i| {
            i.key_pressed(egui::Key::Backspace) || i.key_pressed(egui::Key::Delete)
        });

        if enter && let Some(index) = self.selected {
            self.request_play(index);
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
            if dt > 0.0 && dt < 1.0 {
                if let Some(track) = self.playlist.tracks.get_mut(index) {
                    track.mark_listened(dt);
                    if track.qualifies_as_played() {
                        self.was_playing = true;
                    }
                }
            }
        }

        self.last_listen_at = Some(now);
    }

    fn maybe_autoplay_next(&mut self) {
        let status = self.playback.status();
        let playing = status.is_playing();
        let duration = status.duration_secs();
        let position = status.position_secs();
        let near_end = duration > 0.5 && position >= duration - 0.08;

        // Only chain to next track if this one was actually listened to (≥20s).
        if self.was_playing && !playing && near_end && self.autoplay {
            self.was_playing = false;
            self.last_listen_at = None;
            self.play_next();
        }
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
                if let Some(path) = &dropped.path {
                    if path.is_dir() {
                        folders.push(path.clone());
                    } else {
                        files.push(path.clone());
                    }
                }
            }
        });

        if let Some(folder) = folders.into_iter().next() {
            self.load_folder(folder);
        } else if !files.is_empty() {
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

                // Fixed slot per knob — nested layouts were stacking all four on top of each other.
                ui.allocate_ui(egui::vec2(58.0, 78.0), |ui| {
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
        });
    }

    fn draw_waveform(&mut self, ui: &mut egui::Ui) {
        let index = match self.playing_track.or(self.selected) {
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

        painter.rect_filled(rect, 4.0, egui::Color32::from_gray(28));

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
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
        }

        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let fraction = ((pos.x - rect.left()) / width).clamp(0.0, 1.0) as f64;
                self.seek_from_waveform_click(fraction);
            }
        }
    }

    fn draw_title_cell(&self, ui: &mut egui::Ui, index: usize, width: f32) -> egui::Response {
        let track = &self.playlist.tracks[index];
        let selected = self.selected == Some(index);
        let playing = self.playing_track == Some(index);
        let label = if playing {
            format!("▶ {}", track.title)
        } else {
            track.title.clone()
        };

        let fill = match &track.analysis {
            AnalysisState::Failed(_) => egui::Color32::from_rgb(90, 30, 30),
            AnalysisState::Done => egui::Color32::from_rgb(35, 70, 45),
            AnalysisState::Loading | AnalysisState::Analyzing => egui::Color32::from_rgb(80, 70, 20),
            AnalysisState::Pending => egui::Color32::from_gray(45),
        };

        let progress = track.analysis_progress();
        let animate = matches!(
            track.analysis,
            AnalysisState::Loading | AnalysisState::Analyzing
        );

        let desired = egui::vec2(width, 22.0);
        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::CLICK);

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, egui::Color32::from_gray(28));
        let filled = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width() * progress, rect.height()),
        );
        painter.rect_filled(filled, 3.0, fill);

        if selected {
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.5, egui::Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }

        if animate {
            let pulse = ((ui.input(|i| i.time) as f32 * 3.0).sin() * 0.5 + 0.5) * 40.0;
            painter.rect_stroke(
                rect,
                3.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(120 + pulse as u8)),
                egui::StrokeKind::Outside,
            );
        }

        painter.text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        );

        response
            .on_hover_text(track.filename_label())
    }
}

impl eframe::App for DjApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.playback.poll();
        self.poll_worker();
        self.handle_drops(ctx);
        self.handle_keyboard(ctx);

        let now = ctx.input(|i| i.time);
        self.accumulate_listen_time(now);
        self.maybe_autoplay_next();

        let status = self.playback.status();
        if status.is_playing() {
            self.seek_slider = status.position_secs();
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
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

        egui::Panel::top("top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Load Folder").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    self.load_folder(folder);
                }

                if ui.button("▶ Play/Pause").clicked() {
                    self.toggle_playback();
                }

                ui.checkbox(&mut self.autoplay, "Autoplay");

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

                ui.separator();

                ui.label("Vol");
                if ui
                    .add(egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false))
                    .changed()
                {
                    let _ = self.playback.set_volume(self.volume);
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            self.draw_waveform(ui);
            ui.separator();

            ui.heading("Playlist");
            ui.label(
                egui::RichText::new(
                    "Click = select · Double-click / Enter = play · Backspace = remove from list",
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

            // Reserve fixed widths for meta columns; Title takes the rest (full window).
            let avail = ui.available_width();
            let spacing = 12.0;
            let fixed = 36.0  // #
                + 160.0 // Artist
                + 56.0  // Duration
                + 48.0  // BPM
                + 80.0  // Key
                + 64.0  // Camelot
                + 52.0  // Played
                + spacing * 7.0;
            let title_w = (avail - fixed).max(220.0);

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
                            ui.label(egui::RichText::new("#").strong());
                            ui.label(egui::RichText::new("Artist").strong());
                            ui.label(egui::RichText::new("Title").strong());
                            ui.label(egui::RichText::new("Duration").strong());
                            ui.label(egui::RichText::new("BPM").strong());
                            ui.label(egui::RichText::new("Key").strong());
                            ui.label(egui::RichText::new("Camelot").strong());
                            ui.label(egui::RichText::new("Played").strong());
                            ui.end_row();

                            for index in 0..self.playlist.tracks.len() {
                                let track = &self.playlist.tracks[index];
                                ui.label(track.track_number_label());
                                ui.add(
                                    egui::Label::new(&track.artist)
                                        .truncate(),
                                );

                                let response = self.draw_title_cell(ui, index, title_w);
                                if response.double_clicked() {
                                    pending_play = Some(index);
                                } else if response.clicked() {
                                    pending_select = Some(index);
                                }

                                let track = &self.playlist.tracks[index];
                                ui.label(track.duration_label());

                                let value_color = match &track.analysis {
                                    AnalysisState::Failed(_) => egui::Color32::RED,
                                    AnalysisState::Done => egui::Color32::LIGHT_GREEN,
                                    AnalysisState::Loading | AnalysisState::Analyzing => {
                                        egui::Color32::YELLOW
                                    }
                                    AnalysisState::Pending => egui::Color32::GRAY,
                                };

                                ui.label(egui::RichText::new(track.bpm_label()).color(value_color));
                                ui.label(egui::RichText::new(track.key_label()).color(value_color));
                                ui.label(
                                    egui::RichText::new(track.camelot_label())
                                        .color(value_color)
                                        .strong(),
                                );
                                ui.label(if track.played { "✓" } else { "—" });
                                ui.end_row();
                            }
                        });
                });

            if let Some(index) = pending_play {
                self.request_play(index);
            } else if let Some(index) = pending_select {
                self.select_track(index);
            }
        });
    }
}

fn format_clock(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let minutes = total / 60;
    let seconds = total % 60;
    format!("{minutes}:{seconds:02}")
}
