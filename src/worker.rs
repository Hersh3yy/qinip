use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::decode::{decode_to_samples, DecodedAudio};
use crate::dev;
use crate::eq::{band_waveform_peaks, BandWaveform};

/// Waveform resolution: min/max peaks per band across the track.
const WAVEFORM_PEAKS: usize = 800;

#[derive(Debug)]
pub enum WorkerJob {
    LoadTrack {
        path: PathBuf,
        autoplay: bool,
    },
}

#[derive(Debug)]
pub enum WorkerUpdate {
    TrackReady {
        path: PathBuf,
        audio: DecodedAudio,
        /// Computed here, not on the UI thread: four filters over a whole
        /// track take long enough to stall a frame.
        peaks: BandWaveform,
        autoplay: bool,
    },
    Failed {
        path: PathBuf,
        message: String,
    },
}

pub struct AnalysisWorker {
    job_tx: Sender<WorkerJob>,
    _handle: JoinHandle<()>,
}

impl AnalysisWorker {
    pub fn spawn() -> (Self, Receiver<WorkerUpdate>) {
        let (job_tx, job_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();

        let handle = thread::spawn(move || worker_loop(job_rx, update_tx));

        (
            Self {
                job_tx,
                _handle: handle,
            },
            update_rx,
        )
    }

    pub fn enqueue(&self, job: WorkerJob) {
        let _ = self.job_tx.send(job);
    }
}

fn worker_loop(job_rx: Receiver<WorkerJob>, update_tx: Sender<WorkerUpdate>) {
    // Play requests go first, in the order they were made; a path already
    // waiting is not queued twice.
    let mut queue: VecDeque<WorkerJob> = VecDeque::new();
    let mut priority: VecDeque<WorkerJob> = VecDeque::new();

    loop {
        while let Ok(job) = job_rx.try_recv() {
            let path = job_path(&job);
            if job_is_autoplay(&job) {
                queue.retain(|j| job_path(j) != path);
                if !priority.iter().any(|j| job_path(j) == path) {
                    priority.push_back(job);
                }
            } else if !queue.iter().chain(priority.iter()).any(|j| job_path(j) == path) {
                queue.push_back(job);
            }
        }

        let job = match priority.pop_front().or_else(|| queue.pop_front()) {
            Some(job) => job,
            None => {
                thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
        };

        let WorkerJob::LoadTrack { path, autoplay } = job;

        let path_display = path.display().to_string();
        dev::log(format!("decoding {path_display}"));

        let start = std::time::Instant::now();
        let audio = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_to_samples(&path_display)
        })) {
            Ok(Ok(audio)) => audio,
            Ok(Err(message)) => {
                let _ = update_tx.send(WorkerUpdate::Failed { path, message });
                continue;
            }
            Err(_) => {
                let _ = update_tx.send(WorkerUpdate::Failed {
                    path,
                    message: "decode panicked".to_string(),
                });
                continue;
            }
        };
        dev::log_timing(&format!("decode {path_display}"), start);

        let peak_start = std::time::Instant::now();
        let peaks = band_waveform_peaks(&audio.downmix_to_mono(), audio.sample_rate, WAVEFORM_PEAKS);
        dev::log_timing("band waveform peaks", peak_start);

        if update_tx
            .send(WorkerUpdate::TrackReady {
                path,
                audio,
                peaks,
                autoplay,
            })
            .is_err()
        {
            return;
        }
    }
}

fn job_is_autoplay(job: &WorkerJob) -> bool {
    matches!(job, WorkerJob::LoadTrack { autoplay: true, .. })
}

fn job_path(job: &WorkerJob) -> &std::path::Path {
    match job {
        WorkerJob::LoadTrack { path, .. } => path,
    }
}
