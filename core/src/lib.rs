//! Swift Army Knife — media engine.
//!
//! Pure-Rust core that probes a dropped file, decides what can be done with it,
//! builds the ffmpeg command sequence, runs it with progress + cancellation,
//! and manages the output path. The Swift app drives this over FFI (added in
//! M2); everything here is usable and testable without any GUI.

pub mod engine;
mod ffi;
mod jobs;
pub mod ops;
pub mod output;
pub mod probe;

use engine::{run_stages, EngineError, Progress};
use ops::{op_for, JobParams, OpId};
use probe::ProbeResult;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One entry in the preset menu the UI shows after a drop.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub op_id: u32,
    pub label: String,
}

/// Which operations apply to a probed file: the video ops for a video, the
/// image ops for a still, or an empty menu for anything else.
pub fn menu_for(probe: &ProbeResult) -> Vec<MenuItem> {
    let ids: &[OpId] = if probe.is_video {
        &[OpId::Convert, OpId::Compress, OpId::ExtractAudio, OpId::Gif]
    } else if probe.is_image {
        &[OpId::ImageConvert, OpId::ImageResize, OpId::ImageCompress]
    } else {
        &[]
    };
    ids.iter()
        .map(|&id| {
            let op = op_for(id);
            MenuItem {
                op_id: id as u32,
                label: op.label().to_string(),
            }
        })
        .collect()
}

/// Where an operation's output will be written (collision-free, next to source).
pub fn plan_output(input: &str, op_id: u32, params: &JobParams) -> Result<PathBuf, EngineError> {
    let id = OpId::from_u32(op_id)
        .ok_or_else(|| EngineError::BadRequest(format!("unknown operation {op_id}")))?;
    let op = op_for(id);
    Ok(output::output_path(
        Path::new(input),
        &op.output_suffix(params),
        &op.output_ext(input, params),
    ))
}

/// A unique scratch dir for one job's intermediate files (palette, pass logs).
fn make_workdir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("sak-{}-{}", std::process::id(), n));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Run a job to completion (blocking). Returns the output path on success.
///
/// On failure or cancellation any partially-written output is removed so a
/// broken file is never left behind. The scratch workdir is always cleaned up.
#[allow(clippy::too_many_arguments)]
pub fn run_job_blocking(
    ffmpeg_bin: &str,
    input: &str,
    op_id: u32,
    params: &JobParams,
    probe: &ProbeResult,
    cancel: &AtomicBool,
    on_progress: impl FnMut(Progress),
) -> Result<PathBuf, EngineError> {
    let id = OpId::from_u32(op_id)
        .ok_or_else(|| EngineError::BadRequest(format!("unknown operation {op_id}")))?;
    let op = op_for(id);

    let output = output::output_path(
        Path::new(input),
        &op.output_suffix(params),
        &op.output_ext(input, params),
    );
    let output_str = output.to_string_lossy().into_owned();

    let workdir = make_workdir();
    let stages = op.build_stages(input, &output_str, &workdir, probe, params);

    let result = run_stages(ffmpeg_bin, &stages, probe.duration_s, cancel, on_progress);

    // Always tidy the scratch dir.
    let _ = std::fs::remove_dir_all(&workdir);

    match result {
        Ok(()) => Ok(output),
        Err(e) => {
            // Don't leave a broken/partial output behind.
            let _ = std::fs::remove_file(&output);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_empty_for_non_video() {
        assert!(menu_for(&ProbeResult {
            is_video: false,
            is_image: false,
            duration_s: 0.0,
            width: 0,
            height: 0,
            video_codec: String::new(),
            has_audio: false,
            audio_codec: String::new(),
        })
        .is_empty());
    }

    #[test]
    fn menu_has_four_ops_for_video() {
        let m = menu_for(&video_probe());
        assert_eq!(m.len(), 4);
        assert_eq!(m[0].label, "Convert to MP4");
    }

    #[test]
    fn plan_output_uses_correct_extension() {
        let out = plan_output(
            "/v/clip.webm",
            OpId::ExtractAudio as u32,
            &JobParams::default(),
        )
        .unwrap();
        assert_eq!(out.extension().unwrap(), "mp3");
        let bad = plan_output("/v/clip.webm", 99, &JobParams::default());
        assert!(bad.is_err());
    }

    fn video_probe() -> ProbeResult {
        ProbeResult {
            is_video: true,
            is_image: false,
            duration_s: 2.0,
            width: 320,
            height: 240,
            video_codec: "h264".into(),
            has_audio: true,
            audio_codec: "aac".into(),
        }
    }

    // ---- integration: exercises the whole path against real ffmpeg ----------
    // Skipped automatically if ffmpeg/ffprobe aren't on PATH.

    fn tool_available(bin: &str) -> bool {
        std::process::Command::new(bin)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn end_to_end_all_ops_with_real_ffmpeg() {
        if !tool_available("ffmpeg") || !tool_available("ffprobe") {
            eprintln!("skipping: ffmpeg/ffprobe not on PATH");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.mp4");
        let input_str = input.to_string_lossy().into_owned();

        // Synthesize a 2s test clip with audio.
        let gen = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=2:size=320x240:rate=15",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=2",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-shortest",
                &input_str,
            ])
            .output()
            .unwrap();
        assert!(gen.status.success(), "test clip generation failed");

        let p = probe::probe("ffprobe", &input_str);
        assert!(p.is_video);
        assert!(p.has_audio);
        assert!((p.duration_s - 2.0).abs() < 0.5);

        let cancel = AtomicBool::new(false);
        for op_id in [
            OpId::Convert as u32,
            OpId::Compress as u32,
            OpId::ExtractAudio as u32,
            OpId::Gif as u32,
        ] {
            let mut last_pct = 0.0f32;
            let params = if op_id == OpId::Compress as u32 {
                JobParams {
                    target_mb: 1.0,
                    ..Default::default()
                }
            } else {
                JobParams::default()
            };
            let out = run_job_blocking("ffmpeg", &input_str, op_id, &params, &p, &cancel, |pr| {
                last_pct = pr.pct
            })
            .unwrap_or_else(|e| panic!("op {op_id} failed: {e}"));

            let meta = std::fs::metadata(&out)
                .unwrap_or_else(|_| panic!("op {op_id} produced no output at {out:?}"));
            assert!(meta.len() > 0, "op {op_id} produced empty output");
            assert!(
                (last_pct - 1.0).abs() < 1e-3,
                "op {op_id} didn't reach 100%"
            );
        }
    }
}
