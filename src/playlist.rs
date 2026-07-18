use std::collections::HashSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::decode::probe_metadata;
use crate::key::to_camelot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisState {
    Pending,
    Loading,
    Analyzing,
    Done,
    Failed(String),
}

pub struct TrackEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub track_number: Option<u32>,
    pub duration_secs: Option<f64>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub camelot: Option<String>,
    pub analysis: AnalysisState,
    /// Cumulative seconds this track has actually been playing (across pauses).
    pub listened_secs: f64,
    pub played: bool,
}

impl TrackEntry {
    pub fn from_path(path: PathBuf) -> Self {
        let meta = probe_metadata(&path);
        Self {
            path,
            title: meta.title,
            artist: meta.artist,
            track_number: meta.track_number,
            duration_secs: meta.duration_secs,
            bpm: None,
            key: None,
            camelot: None,
            analysis: AnalysisState::Pending,
            listened_secs: 0.0,
            played: false,
        }
    }

    /// Seconds of playback before we mark a track as "played" and allow autoplay chain.
    pub const PLAYED_THRESHOLD_SECS: f64 = 20.0;

    pub fn mark_listened(&mut self, additional_secs: f64) {
        if additional_secs <= 0.0 {
            return;
        }
        self.listened_secs += additional_secs;
        if self.listened_secs >= Self::PLAYED_THRESHOLD_SECS {
            self.played = true;
        }
    }

    pub fn qualifies_as_played(&self) -> bool {
        self.played || self.listened_secs >= Self::PLAYED_THRESHOLD_SECS
    }

    pub fn display_label(&self) -> String {
        if self.artist != "Unknown Artist" {
            format!("{} — {}", self.artist, self.title)
        } else {
            self.title.clone()
        }
    }

    pub fn track_number_label(&self) -> String {
        self.track_number
            .map(|n| n.to_string())
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn filename_label(&self) -> String {
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    }

    pub fn duration_label(&self) -> String {
        self.duration_secs
            .map(format_duration)
            .unwrap_or_else(|| "--:--".to_string())
    }

    pub fn bpm_label(&self) -> String {
        match (&self.analysis, self.bpm) {
            (AnalysisState::Failed(msg), _) => format!("err: {msg}"),
            (AnalysisState::Loading | AnalysisState::Analyzing, _) => "…".to_string(),
            (_, Some(bpm)) => format!("{bpm:.0}"),
            _ => "—".to_string(),
        }
    }

    pub fn key_label(&self) -> String {
        match (&self.analysis, &self.key) {
            (AnalysisState::Failed(_), _) => "—".to_string(),
            (AnalysisState::Loading | AnalysisState::Analyzing, _) => "…".to_string(),
            (_, Some(key)) => key.clone(),
            _ => "—".to_string(),
        }
    }

    pub fn camelot_label(&self) -> String {
        match (&self.analysis, &self.camelot) {
            (AnalysisState::Failed(_), _) => "—".to_string(),
            (AnalysisState::Loading | AnalysisState::Analyzing, _) => "…".to_string(),
            (_, Some(camelot)) => camelot.clone(),
            _ => "—".to_string(),
        }
    }

    pub fn set_analysis_result(&mut self, bpm: f32, key: String) {
        self.camelot = to_camelot(&key);
        self.bpm = Some(bpm);
        self.key = Some(key);
        self.analysis = AnalysisState::Done;
    }

    pub fn analysis_progress(&self) -> f32 {
        match self.analysis {
            AnalysisState::Pending => 0.05,
            AnalysisState::Loading => 0.35,
            AnalysisState::Analyzing => 0.7,
            AnalysisState::Done => 1.0,
            AnalysisState::Failed(_) => 1.0,
        }
    }
}

pub struct Playlist {
    pub tracks: Vec<TrackEntry>,
}

impl Playlist {
    pub fn new() -> Self {
        Self { tracks: Vec::new() }
    }

    /// Append MP3s from a folder. Skips paths already in the playlist.
    /// Returns how many tracks were added.
    pub fn append_folder(&mut self, folder: &Path) -> usize {
        let mut paths: Vec<PathBuf> = WalkDir::new(folder)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        paths.sort();
        self.append_paths(paths)
    }

    /// Append file paths. Skips duplicates. Returns how many were added.
    pub fn append_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let existing: HashSet<PathBuf> = self.tracks.iter().map(|t| t.path.clone()).collect();
        let mut paths: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| {
                p.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
            })
            .filter(|p| !existing.contains(p))
            .collect();

        paths.sort();
        let added = paths.len();
        self.tracks
            .extend(paths.into_iter().map(TrackEntry::from_path));
        added
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.tracks.iter().position(|t| t.path == path)
    }

    pub fn remove_at(&mut self, index: usize) -> Option<TrackEntry> {
        if index < self.tracks.len() {
            Some(self.tracks.remove(index))
        } else {
            None
        }
    }
}

fn format_duration(secs: f64) -> String {
    let total = secs.round() as u64;
    let minutes = total / 60;
    let seconds = total % 60;
    format!("{minutes}:{seconds:02}")
}
