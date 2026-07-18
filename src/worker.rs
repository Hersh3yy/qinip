use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use crate::bpm::detect_bpm;
use crate::decode::{decode_to_samples, DecodedAudio};
use crate::key::detect_key;

#[derive(Debug)]
pub enum WorkerJob {
    LoadAndAnalyze {
        track_index: usize,
        path: PathBuf,
        autoplay: bool,
    },
}

#[derive(Debug)]
pub enum WorkerUpdate {
    TrackReady {
        track_index: usize,
        audio: DecodedAudio,
        autoplay: bool,
    },
    AnalysisComplete {
        track_index: usize,
        bpm: f32,
        key: String,
    },
    Failed {
        track_index: usize,
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

    pub fn enqueue_all(&self, jobs: impl IntoIterator<Item = WorkerJob>) {
        for job in jobs {
            self.enqueue(job);
        }
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

        let WorkerJob::LoadAndAnalyze {
            track_index,
            path,
            autoplay,
        } = job;

        let path_display = path.display().to_string();
        let audio = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_to_samples(&path_display)
        })) {
            Ok(audio) => audio,
            Err(_) => {
                let _ = update_tx.send(WorkerUpdate::Failed {
                    track_index,
                    message: "decode panicked".to_string(),
                });
                continue;
            }
        };

        if update_tx
            .send(WorkerUpdate::TrackReady {
                track_index,
                audio: DecodedAudio {
                    samples: audio.samples.clone(),
                    sample_rate: audio.sample_rate,
                    channels: audio.channels,
                },
                autoplay,
            })
            .is_err()
        {
            return;
        }

        let bpm_result = detect_bpm(&audio);
        let key_result = detect_key(&audio);

        if update_tx
            .send(WorkerUpdate::AnalysisComplete {
                track_index,
                bpm: bpm_result.bpm,
                key: key_result.key_name,
            })
            .is_err()
        {
            return;
        }
    }
}

fn job_is_autoplay(job: &WorkerJob) -> bool {
    matches!(
        job,
        WorkerJob::LoadAndAnalyze {
            autoplay: true,
            ..
        }
    )
}
