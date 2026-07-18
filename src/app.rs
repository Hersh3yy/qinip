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
use crate::playback::PlaybackEngine;
use crate::playlist::{AnalysisState, Playlist};
use crate::worker::{AnalysisWorker, WorkerJob, WorkerUpdate};

struct AnalysisResult {
    track_index: usize,
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
    audio_cache: HashMap<usize, Arc<DecodedAudio>>,
    waveform_cache: HashMap<usize, BandWaveform>,
    selected: Option<usize>,
    playing_track: Option<usize>,
    volume: f32,
    eq: EqGains,
    seek_slider: f64,
    status_message: String,
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
        }
    }

    fn poll_worker(&mut self) {
        while let Ok(update) = self.updates_rx.try_recv() {
            match update {
                WorkerUpdate::TrackReady {
                    track_index,
                    audio,
                    autoplay,
                } => {
                    dev::log(format!(
                        "track #{track_index} ready — {:.1}s, {} samples",
                        audio.duration_secs(),
                        audio.samples.len()
                    ));

                    let peak_start = std::time::Instant::now();
                    let mono = audio.downmix_to_mono();
                    let peaks = band_waveform_peaks(&mono, audio.sample_rate, 800);
                    self.waveform_cache.insert(track_index, peaks);
                    dev::log_timing("band waveform peaks", peak_start);

                    if let Some(track) = self.playlist.tracks.get_mut(track_index) {
                        track.analysis = AnalysisState::Analyzing;
                        track.duration_secs = Some(audio.duration_secs());
                    }

                    let audio = Arc::new(audio);
                    self.audio_cache.insert(track_index, Arc::clone(&audio));
                    self.spawn_analysis(track_index, audio);

                    if autoplay {
                        self.play_track(track_index, true);
                    }
                }
                WorkerUpdate::Failed {
                    track_index,
                    message,
                } => {
                    if let Some(track) = self.playlist.tracks.get_mut(track_index) {
                        track.analysis = AnalysisState::Failed(message.clone());
                    }
                    self.status_message = format!("Track #{track_index} failed: {message}");
                    dev::log(format!("track #{track_index} failed: {message}"));
                }
            }
        }

        while let Ok(result) = self.analysis_rx.try_recv() {
            if let Some(track) = self.playlist.tracks.get_mut(result.track_index) {
                track.set_analysis_result(result.bpm, result.key.clone());
                dev::log(format!(
                    "track #{} analysis done — {:.1} BPM, {} ({})",
                    result.track_index,
                    result.bpm,
                    result.key,
                    track.camelot.as_deref().unwrap_or("?")
                ));
            }
        }
    }

    fn spawn_analysis(&self, track_index: usize, audio: Arc<DecodedAudio>) {
        let tx = self.analysis_tx.clone();
        thread::spawn(move || {
            let start = std::time::Instant::now();
            let bpm_result = detect_bpm(&audio);
            let key_result = detect_key(&audio);
            dev::log_timing(&format!("bpm+key track #{track_index}"), start);

            let _ = tx.send(AnalysisResult {
                track_index,
                bpm: bpm_result.bpm,
                key: key_result.key_name,
            });
        });
    }

    fn load_folder(&mut self, folder: PathBuf) {
        self.playlist.load_folder(&folder);
        self.selected = None;
        self.playing_track = None;
        self.audio_cache.clear();
        self.waveform_cache.clear();
        let count = self.playlist.tracks.len();
        self.status_message = format!(
            "Loaded {count} tracks from {} — analyzing all…",
            folder.display()
        );
        dev::log(format!("folder loaded: {}", folder.display()));
        self.analyze_all();
    }

    fn load_paths(&mut self, paths: Vec<PathBuf>) {
        self.playlist.load_paths(paths);
        self.selected = None;
        self.playing_track = None;
        self.audio_cache.clear();
        self.waveform_cache.clear();
        let count = self.playlist.tracks.len();
        self.status_message = format!("Loaded {count} tracks — analyzing all…");
        self.analyze_all();
    }

    fn analyze_all(&mut self) {
        let jobs: Vec<_> = self
            .playlist
            .tracks
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.audio_cache.contains_key(index))
            .map(|(index, track)| {
                (
                    index,
                    WorkerJob::LoadTrack {
                        track_index: index,
                        path: track.path.clone(),
                        autoplay: false,
                    },
                )
            })
            .collect();

        for (index, _) in &jobs {
            if let Some(entry) = self.playlist.tracks.get_mut(*index) {
                entry.analysis = AnalysisState::Loading;
            }
        }

        for (_, job) in jobs {
            self.worker.enqueue(job);
        }

        self.status_message = "Analyzing all tracks in background…".to_string();
        dev::log("queued analyze-all");
    }

    fn select_track(&mut self, index: usize) {
        self.selected = Some(index);

        if self.audio_cache.contains_key(&index) {
            self.play_track(index, true);
            return;
        }

        if let Some(track) = self.playlist.tracks.get_mut(index) {
            track.analysis = AnalysisState::Loading;
            let path = track.path.clone();
            let label = track.display_label();
            self.worker.enqueue(WorkerJob::LoadTrack {
                track_index: index,
                path,
                autoplay: true,
            });
            self.status_message = format!("Loading {label}…");
            dev::log(format!("selected track #{index}: {label}"));
        }
    }

    fn play_track(&mut self, index: usize, restart: bool) {
        let Some(audio) = self.audio_cache.get(&index).map(|a| (**a).clone()) else {
            dev::log(format!("play_track #{index} — not in cache yet"));
            return;
        };

        let _ = self.playback.load(audio);
        if restart {
            let _ = self.playback.seek_secs(0.0);
        }
        let _ = self.playback.set_volume(self.volume);
        let _ = self.playback.play();

        self.playing_track = Some(index);
        self.selected = Some(index);
        self.status_message = format!(
            "Playing {}",
            self.playlist.tracks[index].display_label()
        );
        dev::log(format!("playing track #{index}"));
    }

    fn toggle_playback(&mut self) {
        let status = self.playback.status();
        if status.is_playing() {
            let _ = self.playback.pause();
        } else if let Some(index) = self.playing_track {
            if status.position_secs() >= status.duration_secs().max(0.001) - 0.05 {
                self.play_track(index, true);
            } else {
                let _ = self.playback.play();
            }
        } else if let Some(index) = self.selected {
            self.select_track(index);
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
            let mut changed = false;
            let gains = [
                (&mut self.eq.low, BAND_LABELS[0], BAND_COLORS[0]),
                (&mut self.eq.low_mid, BAND_LABELS[1], BAND_COLORS[1]),
                (&mut self.eq.hi_mid, BAND_LABELS[2], BAND_COLORS[2]),
                (&mut self.eq.hi, BAND_LABELS[3], BAND_COLORS[3]),
            ];

            for (gain, label, color) in gains {
                let (r, g, b) = color;
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .color(egui::Color32::from_rgb(r, g, b))
                            .small(),
                    );
                    if ui
                        .add(
                            egui::Slider::new(gain, 0.0..=2.0)
                                .vertical()
                                .show_value(false),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    ui.label(format!("{:.0}%", *gain * 100.0));
                });
                ui.add_space(8.0);
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

        let Some(bands) = self.waveform_cache.get(&index).cloned() else {
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

        // Draw bands bottom→top so highs sit visually on top of lows.
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
}

impl eframe::App for DjApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.playback.poll();
        self.poll_worker();
        self.handle_drops(ctx);

        let status = self.playback.status();
        if status.is_playing() {
            self.seek_slider = status.position_secs();
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
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

                if ui.button("Analyze All").clicked() {
                    self.analyze_all();
                }

                if ui.button("▶ Play/Pause").clicked() {
                    self.toggle_playback();
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
            ui.separator();

            if self.playlist.tracks.is_empty() {
                ui.label("Drop a folder of MP3s here, or use Load Folder.");
                return;
            }

            let mut pending_select = None;

            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("playlist_grid")
                    .num_columns(6)
                    .spacing([12.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Track").strong());
                        ui.label(egui::RichText::new("Duration").strong());
                        ui.label(egui::RichText::new("BPM").strong());
                        ui.label(egui::RichText::new("Key").strong());
                        ui.label(egui::RichText::new("Camelot").strong());
                        ui.label(egui::RichText::new("Status").strong());
                        ui.end_row();

                        for index in 0..self.playlist.tracks.len() {
                            let track = &self.playlist.tracks[index];
                            let selected = self.selected == Some(index);
                            let playing = self.playing_track == Some(index);

                            let label = if playing {
                                format!("▶ {}", track.display_label())
                            } else {
                                track.display_label()
                            };

                            let response = ui.selectable_label(selected, label);
                            if response.clicked() {
                                pending_select = Some(index);
                            }

                            ui.label(track.duration_label());

                            let bpm_text = track.bpm_label();
                            let key_text = track.key_label();
                            let camelot_text = track.camelot_label();
                            let status_text = track.status_label();

                            let value_color = match &track.analysis {
                                AnalysisState::Failed(_) => egui::Color32::RED,
                                AnalysisState::Done => egui::Color32::LIGHT_GREEN,
                                AnalysisState::Loading | AnalysisState::Analyzing => {
                                    egui::Color32::YELLOW
                                }
                                AnalysisState::Pending => egui::Color32::GRAY,
                            };

                            ui.label(egui::RichText::new(bpm_text).color(value_color));
                            ui.label(egui::RichText::new(key_text).color(value_color));
                            ui.label(
                                egui::RichText::new(camelot_text)
                                    .color(value_color)
                                    .strong(),
                            );
                            ui.label(egui::RichText::new(status_text).color(value_color));
                            ui.end_row();
                        }
                    });
            });

            if let Some(index) = pending_select {
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
