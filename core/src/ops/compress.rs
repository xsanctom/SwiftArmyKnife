//! Shrink a video — either to hit a target file size (two-pass) or to a quality
//! level (single-pass CRF).

use super::{base_args, progress_args, CompressMode, JobParams, Op, OpId, Stage};
use crate::probe::ProbeResult;
use std::path::Path;

pub struct Compress;

/// Audio bitrate (kbps) we budget for in target-size mode.
const AUDIO_KBPS: f64 = 128.0;
/// Floor so a tiny target can't drive the video bitrate to something unusable.
const MIN_VIDEO_KBPS: f64 = 100.0;

/// Work out the video bitrate (kbps) to hit `target_mb` over `duration_s`,
/// leaving room for audio and a little container overhead.
///
/// Pulled out as a pure function so the arithmetic is unit-tested directly.
pub fn target_video_kbps(target_mb: f64, duration_s: f64, has_audio: bool) -> u32 {
    if duration_s <= 0.0 {
        return MIN_VIDEO_KBPS as u32;
    }
    // MB defined as 1_000_000 bytes (matches how upload limits are quoted).
    let total_kbps = target_mb * 8000.0 / duration_s;
    let audio = if has_audio { AUDIO_KBPS } else { 0.0 };
    let video = total_kbps * 0.97 - audio; // 3% headroom for muxing overhead
    video.max(MIN_VIDEO_KBPS).round() as u32
}

impl Op for Compress {
    fn id(&self) -> OpId {
        OpId::Compress
    }
    fn label(&self) -> &'static str {
        "Compress"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        "compressed".into()
    }
    fn output_ext(&self, _input: &str, _params: &JobParams) -> String {
        "mp4".into()
    }

    fn build_stages(
        &self,
        input: &str,
        output: &str,
        workdir: &Path,
        probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        match params.compress_mode {
            CompressMode::Crf => {
                // Single-pass quality-based shrink.
                let mut args = base_args(input);
                args.extend(progress_args());
                args.extend([
                    "-c:v".into(),
                    "libx264".into(),
                    "-crf".into(),
                    params.crf.to_string(),
                    "-preset".into(),
                    "medium".into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                ]);
                if probe.has_audio {
                    args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "128k".into()]);
                } else {
                    args.push("-an".into());
                }
                args.extend(["-movflags".into(), "+faststart".into()]);
                args.push(output.into());
                vec![Stage { args, weight: 1.0 }]
            }
            CompressMode::TargetSize => {
                let vk = target_video_kbps(params.target_mb, probe.duration_s, probe.has_audio);
                let passlog = workdir.join("passlog").to_string_lossy().into_owned();

                // Pass 1: analyse, no audio, discard output.
                let mut p1 = base_args(input);
                p1.extend(progress_args());
                p1.extend([
                    "-c:v".into(),
                    "libx264".into(),
                    "-b:v".into(),
                    format!("{vk}k"),
                    "-pass".into(),
                    "1".into(),
                    "-passlogfile".into(),
                    passlog.clone(),
                    "-an".into(),
                    "-f".into(),
                    "null".into(),
                    "/dev/null".into(),
                ]);

                // Pass 2: real encode with audio.
                let mut p2 = base_args(input);
                p2.extend(progress_args());
                p2.extend([
                    "-c:v".into(),
                    "libx264".into(),
                    "-b:v".into(),
                    format!("{vk}k"),
                    "-pass".into(),
                    "2".into(),
                    "-passlogfile".into(),
                    passlog,
                    "-preset".into(),
                    "medium".into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                ]);
                if probe.has_audio {
                    p2.extend([
                        "-c:a".into(),
                        "aac".into(),
                        "-b:a".into(),
                        format!("{}k", AUDIO_KBPS as u32),
                    ]);
                } else {
                    p2.push("-an".into());
                }
                p2.extend(["-movflags".into(), "+faststart".into()]);
                p2.push(output.into());

                vec![
                    Stage {
                        args: p1,
                        weight: 0.5,
                    },
                    Stage {
                        args: p2,
                        weight: 0.5,
                    },
                ]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(duration: f64, has_audio: bool) -> ProbeResult {
        ProbeResult {
            is_video: true,
            is_image: false,
            duration_s: duration,
            width: 1920,
            height: 1080,
            video_codec: "h264".into(),
            has_audio,
            audio_codec: if has_audio {
                "aac".into()
            } else {
                String::new()
            },
        }
    }

    #[test]
    fn bitrate_math_leaves_room_for_audio() {
        // 25 MB over 60s = 200 kbps total*8000/60 ≈ 3333 kbps; minus audio 128, *0.97.
        let vk = target_video_kbps(25.0, 60.0, true);
        let expected = (25.0_f64 * 8000.0 / 60.0 * 0.97 - 128.0).round() as u32;
        assert_eq!(vk, expected);
        assert!(vk > MIN_VIDEO_KBPS as u32);
    }

    #[test]
    fn bitrate_respects_floor_for_tiny_targets() {
        let vk = target_video_kbps(1.0, 600.0, true); // way too small
        assert_eq!(vk, MIN_VIDEO_KBPS as u32);
    }

    #[test]
    fn target_size_is_two_pass() {
        let stages = Compress.build_stages(
            "in.mov",
            "out.mp4",
            Path::new("/wd"),
            &probe(60.0, true),
            &JobParams::default(),
        );
        assert_eq!(stages.len(), 2);
        assert!(stages[0].args.windows(2).any(|w| w == ["-pass", "1"]));
        assert!(stages[1].args.windows(2).any(|w| w == ["-pass", "2"]));
        // Pass 1 discards output and drops audio.
        assert!(stages[0].args.contains(&"-an".to_string()));
        assert!(stages[0].args.contains(&"/dev/null".to_string()));
        // Both passes share one passlog under the workdir.
        assert!(stages[0].args.iter().any(|s| s == "/wd/passlog"));
        assert_eq!(stages[1].args.last().unwrap(), "out.mp4");
        assert!((stages[0].weight + stages[1].weight - 1.0).abs() < 1e-6);
    }

    #[test]
    fn crf_mode_is_single_pass() {
        let params = JobParams {
            compress_mode: CompressMode::Crf,
            crf: 26,
            ..Default::default()
        };
        let stages = Compress.build_stages(
            "in.mov",
            "out.mp4",
            Path::new("/wd"),
            &probe(60.0, true),
            &params,
        );
        assert_eq!(stages.len(), 1);
        assert!(stages[0].args.windows(2).any(|w| w == ["-crf", "26"]));
    }
}
