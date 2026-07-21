use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::decode::{decode_to_samples, DecodedAudio};
use crate::dev;

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
    let mut queue = VecDeque::new();
    let mut priority: Option<WorkerJob> = None;

    loop {
        while let Ok(job) = job_rx.try_recv() {
            if job_is_autoplay(&job) {
                priority = Some(job);
            } else {
                queue.push_back(job);
            }
        }

        let job = if let Some(job) = priority.take() {
            job
        } else {
            match queue.pop_front() {
                Some(job) => job,
                None => {
                    thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }
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

        if update_tx
            .send(WorkerUpdate::TrackReady {
                path,
                audio,
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
