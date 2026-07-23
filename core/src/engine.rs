//! Running ffmpeg stages and reporting progress.
//!
//! The line-parsing ([`parse_progress_seconds`]) is pure and unit-tested; the
//! executor ([`run_stages`]) spawns ffmpeg, streams `-progress` output to
//! compute a percentage + ETA, and supports cooperative cancellation.

use crate::ops::{Stage, Tool};
use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// A progress update handed to the caller's callback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// 0.0..=1.0 across the whole job (all stages).
    pub pct: f32,
    /// Estimated seconds remaining, or `0.0` when not yet known.
    pub eta_s: f64,
}

#[derive(Debug)]
pub enum EngineError {
    Cancelled,
    BadRequest(String),
    Spawn(String),
    Failed { code: Option<i32>, stderr: String },
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Cancelled => write!(f, "Cancelled"),
            EngineError::BadRequest(e) => write!(f, "{e}"),
            EngineError::Spawn(e) => write!(f, "Could not start ffmpeg: {e}"),
            EngineError::Failed { code, stderr } => {
                let tail = last_lines(stderr, 8);
                match code {
                    Some(c) => write!(f, "ffmpeg exited with code {c}\n{tail}"),
                    None => write!(f, "ffmpeg failed\n{tail}"),
                }
            }
        }
    }
}

fn last_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// Parse one `-progress` line into elapsed output seconds, if it is a time line.
///
/// ffmpeg emits several time keys; we accept the microsecond forms
/// (`out_time_us`, and the quirkily-named `out_time_ms` which is *also*
/// microseconds) and the `HH:MM:SS.ffffff` `out_time` form.
pub fn parse_progress_seconds(line: &str) -> Option<f64> {
    let line = line.trim();
    if let Some(v) = line.strip_prefix("out_time_us=") {
        return micros(v);
    }
    if let Some(v) = line.strip_prefix("out_time_ms=") {
        return micros(v); // ffmpeg quirk: out_time_ms is actually microseconds
    }
    if let Some(v) = line.strip_prefix("out_time=") {
        return parse_hms(v.trim());
    }
    None
}

fn micros(v: &str) -> Option<f64> {
    let v = v.trim();
    if v == "N/A" {
        return None;
    }
    v.parse::<f64>().ok().map(|us| us / 1_000_000.0)
}

fn parse_hms(v: &str) -> Option<f64> {
    if v == "N/A" {
        return None;
    }
    let parts: Vec<&str> = v.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let s: f64 = parts[2].parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Run every stage in order, reporting overall progress via `on_progress`.
///
/// - `total_duration_s`: the source duration, used to turn ffmpeg's elapsed
///   output time into a fraction. `0.0` (unknown) yields stage-boundary-only
///   progress rather than a smooth bar.
/// - `cancel`: checked between progress lines; when set, the child is killed
///   and [`EngineError::Cancelled`] is returned.
pub fn run_stages(
    ffmpeg_bin: &str,
    stages: &[Stage],
    total_duration_s: f64,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> Result<(), EngineError> {
    let start = Instant::now();
    let mut completed_weight = 0.0f32;

    for stage in stages {
        if cancel.load(Ordering::Relaxed) {
            return Err(EngineError::Cancelled);
        }

        // Most stages are ffmpeg; image ops may use a sips stage to decode
        // formats ffmpeg mishandles (tiled HEIC/HEIF/AVIF).
        let program = match stage.tool {
            Tool::Ffmpeg => ffmpeg_bin,
            Tool::Sips => "/usr/bin/sips",
            Tool::Python => "/opt/homebrew/bin/python3",
        };
        let mut child = Command::new(program)
            .args(&stage.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| EngineError::Spawn(e.to_string()))?;

        // Drain stderr on a side thread so a chatty/erroring ffmpeg can't block
        // us while we read progress from stdout.
        let mut stderr = child.stderr.take().unwrap();
        let stderr_handle = std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf);
            buf
        });

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stderr_handle.join();
                return Err(EngineError::Cancelled);
            }
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Some(cur) = parse_progress_seconds(&line) {
                let stage_frac = if total_duration_s > 0.0 {
                    (cur / total_duration_s).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                let overall = (completed_weight + stage.weight * stage_frac).clamp(0.0, 1.0);
                on_progress(Progress {
                    pct: overall,
                    eta_s: eta(start.elapsed().as_secs_f64(), overall as f64),
                });
            }
        }

        let status = child
            .wait()
            .map_err(|e| EngineError::Spawn(e.to_string()))?;
        let stderr_text = stderr_handle.join().unwrap_or_default();
        if !status.success() {
            // A cancel could race the exit; treat a killed child as cancelled.
            if cancel.load(Ordering::Relaxed) {
                return Err(EngineError::Cancelled);
            }
            return Err(EngineError::Failed {
                code: status.code(),
                stderr: stderr_text,
            });
        }

        completed_weight += stage.weight;
        on_progress(Progress {
            pct: completed_weight.clamp(0.0, 1.0),
            eta_s: eta(start.elapsed().as_secs_f64(), completed_weight as f64),
        });
    }

    on_progress(Progress {
        pct: 1.0,
        eta_s: 0.0,
    });
    Ok(())
}

fn eta(elapsed_s: f64, overall: f64) -> f64 {
    if overall > 0.02 {
        (elapsed_s * (1.0 - overall) / overall).max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_out_time_us() {
        assert_eq!(parse_progress_seconds("out_time_us=12500000"), Some(12.5));
        assert_eq!(parse_progress_seconds("  out_time_us=1000000 "), Some(1.0));
    }

    #[test]
    fn parses_out_time_ms_as_micros() {
        // The infamous quirk: out_time_ms is microseconds.
        assert_eq!(parse_progress_seconds("out_time_ms=2000000"), Some(2.0));
    }

    #[test]
    fn parses_hms() {
        assert_eq!(
            parse_progress_seconds("out_time=00:00:12.340000"),
            Some(12.34)
        );
        assert_eq!(
            parse_progress_seconds("out_time=00:01:00.000000"),
            Some(60.0)
        );
    }

    #[test]
    fn ignores_non_time_lines_and_na() {
        assert_eq!(parse_progress_seconds("frame=42"), None);
        assert_eq!(parse_progress_seconds("progress=continue"), None);
        assert_eq!(parse_progress_seconds("out_time_us=N/A"), None);
    }

    #[test]
    fn eta_math() {
        // Halfway after 10s → ~10s remaining.
        assert!((eta(10.0, 0.5) - 10.0).abs() < 1e-6);
        // Too early → unknown (0).
        assert_eq!(eta(0.1, 0.001), 0.0);
    }
}
