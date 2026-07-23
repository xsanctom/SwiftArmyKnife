//! Convert to a widely-shareable MP4.

use super::{base_args, progress_args, JobParams, Op, OpId, Stage, VideoCodec};
use crate::probe::ProbeResult;
use std::path::Path;

pub struct Convert;

/// Push the video-codec arguments for the chosen codec / hardware setting.
fn video_codec_args(params: &JobParams, args: &mut Vec<String>) {
    match (params.video_codec, params.hw_accel) {
        (VideoCodec::H264, false) => {
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
        }
        (VideoCodec::Hevc, false) => {
            args.extend([
                "-c:v".into(),
                "libx265".into(),
                "-crf".into(),
                params.crf.to_string(),
                "-preset".into(),
                "medium".into(),
                "-tag:v".into(),
                "hvc1".into(), // so QuickTime/Finder recognise the HEVC track
            ]);
        }
        (VideoCodec::H264, true) => {
            args.extend([
                "-c:v".into(),
                "h264_videotoolbox".into(),
                "-q:v".into(),
                "60".into(),
            ]);
        }
        (VideoCodec::Hevc, true) => {
            args.extend([
                "-c:v".into(),
                "hevc_videotoolbox".into(),
                "-q:v".into(),
                "60".into(),
                "-tag:v".into(),
                "hvc1".into(),
            ]);
        }
    }
}

/// Optional downscale that never upscales (`min(ih, max_height)`).
fn scale_args(params: &JobParams, args: &mut Vec<String>) {
    if let Some(h) = params.max_height {
        args.push("-vf".into());
        // Backslash escapes the comma so the filtergraph parser keeps it inside min().
        args.push(format!("scale=-2:min(ih\\,{h})"));
    }
}

impl Op for Convert {
    fn id(&self) -> OpId {
        OpId::Convert
    }
    fn label(&self) -> &'static str {
        "Convert to MP4"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        String::new()
    }
    fn output_ext(&self, _params: &JobParams) -> String {
        "mp4".into()
    }

    fn build_stages(
        &self,
        input: &str,
        output: &str,
        _workdir: &Path,
        probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let mut args = base_args(input);
        args.extend(progress_args());
        scale_args(params, &mut args);
        video_codec_args(params, &mut args);
        if probe.has_audio {
            args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "192k".into()]);
        } else {
            args.push("-an".into());
        }
        args.extend(["-movflags".into(), "+faststart".into()]);
        args.push(output.into());
        vec![Stage { args, weight: 1.0 }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_with_audio(has_audio: bool) -> ProbeResult {
        ProbeResult {
            is_video: true,
            duration_s: 10.0,
            width: 1920,
            height: 1080,
            video_codec: "vp9".into(),
            has_audio,
            audio_codec: if has_audio { "opus".into() } else { String::new() },
        }
    }

    #[test]
    fn default_h264_with_audio() {
        let stages = Convert.build_stages(
            "in.webm",
            "out.mp4",
            Path::new("/wd"),
            &probe_with_audio(true),
            &JobParams::default(),
        );
        assert_eq!(stages.len(), 1);
        let a = &stages[0].args;
        assert_eq!(a.first().unwrap(), "-hide_banner");
        assert!(a.windows(2).any(|w| w == ["-c:v", "libx264"]));
        assert!(a.windows(2).any(|w| w == ["-crf", "20"]));
        assert!(a.windows(2).any(|w| w == ["-c:a", "aac"]));
        assert!(a.windows(2).any(|w| w == ["-movflags", "+faststart"]));
        assert_eq!(a.last().unwrap(), "out.mp4");
    }

    #[test]
    fn no_audio_uses_an() {
        let stages = Convert.build_stages(
            "in.webm",
            "out.mp4",
            Path::new("/wd"),
            &probe_with_audio(false),
            &JobParams::default(),
        );
        let a = &stages[0].args;
        assert!(a.contains(&"-an".to_string()));
        assert!(!a.windows(2).any(|w| w == ["-c:a", "aac"]));
    }

    #[test]
    fn hevc_and_scale() {
        let params = JobParams {
            video_codec: VideoCodec::Hevc,
            max_height: Some(720),
            ..Default::default()
        };
        let stages = Convert.build_stages(
            "in.mov",
            "out.mp4",
            Path::new("/wd"),
            &probe_with_audio(true),
            &params,
        );
        let a = &stages[0].args;
        assert!(a.windows(2).any(|w| w == ["-c:v", "libx265"]));
        assert!(a.windows(2).any(|w| w == ["-tag:v", "hvc1"]));
        assert!(a.iter().any(|s| s == "scale=-2:min(ih\\,720)"));
    }

    #[test]
    fn hardware_h264_uses_videotoolbox() {
        let params = JobParams {
            hw_accel: true,
            ..Default::default()
        };
        let stages = Convert.build_stages(
            "in.mov",
            "out.mp4",
            Path::new("/wd"),
            &probe_with_audio(true),
            &params,
        );
        let a = &stages[0].args;
        assert!(a.windows(2).any(|w| w == ["-c:v", "h264_videotoolbox"]));
        assert!(!a.iter().any(|s| s == "-crf"));
    }
}
