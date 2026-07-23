//! Background job registry.
//!
//! A job runs ffmpeg on its own thread and publishes progress into a shared
//! snapshot. Swift polls [`poll`] on a timer (cheap, non-blocking) and calls
//! [`cancel`] / [`release`] as needed. This keeps the FFI a handful of plain
//! function calls — no Rust→Swift callbacks across threads.

use crate::engine::{EngineError, Progress};
use crate::ops::JobParams;
use crate::{probe, run_job_blocking};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const STATUS_RUNNING: u32 = 0;
pub const STATUS_DONE: u32 = 1;
pub const STATUS_ERROR: u32 = 2;
pub const STATUS_CANCELLED: u32 = 3;

/// A point-in-time view of a job, cloned out to the caller on each poll.
#[derive(Clone)]
pub struct Snapshot {
    pub pct: f32,
    pub eta_s: f64,
    pub status: u32,
    pub output_path: String,
    pub error: String,
}

impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            pct: 0.0,
            eta_s: 0.0,
            status: STATUS_RUNNING,
            output_path: String::new(),
            error: String::new(),
        }
    }
}

struct JobState {
    cancel: AtomicBool,
    snap: Mutex<Snapshot>,
}

fn registry() -> &'static Mutex<HashMap<u64, Arc<JobState>>> {
    static REG: OnceLock<Mutex<HashMap<u64, Arc<JobState>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(1);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Start a job. Returns a job id to poll/cancel with. Runs on a new thread and
/// returns immediately.
pub fn start(ffmpeg: String, ffprobe: String, path: String, op_id: u32, params: JobParams) -> u64 {
    let id = next_id();
    let state = Arc::new(JobState {
        cancel: AtomicBool::new(false),
        snap: Mutex::new(Snapshot::default()),
    });
    registry().lock().unwrap().insert(id, state.clone());

    std::thread::spawn(move || {
        let p = probe::probe(&ffprobe, &path);
        let progress_state = state.clone();
        let result = run_job_blocking(
            &ffmpeg,
            &path,
            op_id,
            &params,
            &p,
            &state.cancel,
            move |pr: Progress| {
                let mut s = progress_state.snap.lock().unwrap();
                s.pct = pr.pct;
                s.eta_s = pr.eta_s;
            },
        );

        let mut s = state.snap.lock().unwrap();
        match result {
            Ok(out) => {
                s.status = STATUS_DONE;
                s.output_path = out.to_string_lossy().into_owned();
                s.pct = 1.0;
                s.eta_s = 0.0;
            }
            Err(EngineError::Cancelled) => {
                s.status = STATUS_CANCELLED;
            }
            Err(e) => {
                s.status = STATUS_ERROR;
                s.error = e.to_string();
            }
        }
    });

    id
}

/// Current snapshot for a job. Unknown ids report as an error snapshot.
pub fn poll(id: u64) -> Snapshot {
    match registry().lock().unwrap().get(&id) {
        Some(state) => state.snap.lock().unwrap().clone(),
        None => Snapshot {
            status: STATUS_ERROR,
            error: "unknown job".into(),
            ..Default::default()
        },
    }
}

/// Request cancellation. The worker notices, kills ffmpeg, and transitions to
/// the cancelled status; the partial output is cleaned up by the engine.
pub fn cancel(id: u64) {
    if let Some(state) = registry().lock().unwrap().get(&id) {
        state.cancel.store(true, Ordering::Relaxed);
    }
}

/// Drop a finished job's bookkeeping. Safe to call once the UI has read a
/// terminal snapshot.
pub fn release(id: u64) {
    registry().lock().unwrap().remove(&id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn ffmpeg_ok() -> bool {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn cancel_stops_job_and_removes_partial_output() {
        if !ffmpeg_ok() {
            eprintln!("skipping: ffmpeg not on PATH");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("clip.mp4");
        let input_str = input.to_string_lossy().into_owned();

        // A long-ish source so the convert can't finish before we cancel:
        // generated fast (ultrafast), but 60s at 720p takes real time to re-encode.
        let gen = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner", "-y",
                "-f", "lavfi", "-i", "testsrc=duration=60:size=1280x720:rate=30",
                "-c:v", "libx264", "-preset", "ultrafast", &input_str,
            ])
            .output()
            .unwrap();
        assert!(gen.status.success(), "generation failed");

        let id = start("ffmpeg".into(), "ffprobe".into(), input_str.clone(), 0, JobParams::default());

        // Wait for the job to actually start producing progress, then cancel.
        let started = Instant::now();
        loop {
            let s = poll(id);
            if s.pct > 0.0 || s.status != STATUS_RUNNING {
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(30), "job never started");
            std::thread::sleep(Duration::from_millis(20));
        }
        cancel(id);

        // Wait for it to settle into the cancelled state.
        let cancelled_at = Instant::now();
        loop {
            let s = poll(id);
            if s.status != STATUS_RUNNING {
                assert_eq!(s.status, STATUS_CANCELLED, "expected cancelled, got {}", s.status);
                break;
            }
            assert!(cancelled_at.elapsed() < Duration::from_secs(15), "cancel didn't take effect");
            std::thread::sleep(Duration::from_millis(20));
        }

        // The partial output must have been cleaned up.
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("mp4"))
            .filter(|e| e.path() != input)
            .collect();
        assert!(leftover.is_empty(), "partial output not removed: {leftover:?}");

        release(id);
    }
}
