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
    pub duration_secs: Option<f64>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub camelot: Option<String>,
    pub analysis: AnalysisState,
}

impl TrackEntry {
    pub fn from_path(path: PathBuf) -> Self {
        let meta = probe_metadata(&path);
        Self {
            path,
            title: meta.title,
            artist: meta.artist,
            duration_secs: meta.duration_secs,
            bpm: None,
            key: None,
            camelot: None,
            analysis: AnalysisState::Pending,
        }
    }

    pub fn display_label(&self) -> String {
        format!("{} — {}", self.artist, self.title)
    }

    pub fn duration_label(&self) -> String {
        self.duration_secs
            .map(format_duration)
            .unwrap_or_else(|| "--:--".to_string())
    }

    pub fn bpm_label(&self) -> String {
        match (&self.analysis, self.bpm) {
            (AnalysisState::Failed(msg), _) => format!("err: {msg}"),
            (AnalysisState::Loading, _) => "…".to_string(),
            (AnalysisState::Analyzing, _) => "…".to_string(),
            (_, Some(bpm)) => format!("{bpm:.0}"),
            (AnalysisState::Pending, _) => "—".to_string(),
            (AnalysisState::Done, None) => "—".to_string(),
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

    pub fn status_label(&self) -> String {
        match &self.analysis {
            AnalysisState::Pending => "—".to_string(),
            AnalysisState::Loading => "loading".to_string(),
            AnalysisState::Analyzing => "analyzing".to_string(),
            AnalysisState::Done => "done".to_string(),
            AnalysisState::Failed(msg) => format!("err: {msg}"),
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

    pub fn load_folder(&mut self, folder: &Path) {
        self.tracks.clear();

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

        self.tracks = paths.into_iter().map(TrackEntry::from_path).collect();
    }

    pub fn load_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        self.tracks.clear();

        let mut paths: Vec<PathBuf> = paths
            .into_iter()
            .filter(|p| {
                p.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("mp3"))
            })
            .collect();

        paths.sort();
        self.tracks = paths.into_iter().map(TrackEntry::from_path).collect();
    }
}

fn format_duration(secs: f64) -> String {
    let total = secs.round() as u64;
    let minutes = total / 60;
    let seconds = total % 60;
    format!("{minutes}:{seconds:02}")
}
