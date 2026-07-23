//! Extract the audio track to a standalone file.

use super::{base_args, progress_args, AudioFormat, JobParams, Op, OpId, Stage};
use crate::probe::ProbeResult;
use std::path::Path;

pub struct ExtractAudio;

impl Op for ExtractAudio {
    fn id(&self) -> OpId {
        OpId::ExtractAudio
    }
    fn label(&self) -> &'static str {
        "Extract audio"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        String::new()
    }
    fn output_ext(&self, _input: &str, params: &JobParams) -> String {
        match params.audio_format {
            AudioFormat::Mp3 => "mp3".into(),
            AudioFormat::M4a => "m4a".into(),
        }
    }

    fn build_stages(
        &self,
        input: &str,
        output: &str,
        _workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let mut args = base_args(input);
        args.extend(progress_args());
        args.push("-vn".into()); // drop video
        let bitrate = format!("{}k", params.audio_bitrate_k);
        match params.audio_format {
            AudioFormat::Mp3 => {
                args.extend(["-c:a".into(), "libmp3lame".into(), "-b:a".into(), bitrate]);
            }
            AudioFormat::M4a => {
                args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), bitrate]);
            }
        }
        args.push(output.into());
        vec![Stage { args, weight: 1.0 }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> ProbeResult {
        ProbeResult {
            is_video: true,
            is_image: false,
            duration_s: 30.0,
            width: 1280,
            height: 720,
            video_codec: "h264".into(),
            has_audio: true,
            audio_codec: "aac".into(),
        }
    }

    #[test]
    fn default_is_mp3() {
        let stages = ExtractAudio.build_stages(
            "in.mp4",
            "out.mp3",
            Path::new("/wd"),
            &probe(),
            &JobParams::default(),
        );
        let a = &stages[0].args;
        assert!(a.contains(&"-vn".to_string()));
        assert!(a.windows(2).any(|w| w == ["-c:a", "libmp3lame"]));
        assert!(a.windows(2).any(|w| w == ["-b:a", "192k"]));
        assert_eq!(
            ExtractAudio.output_ext("in.mp4", &JobParams::default()),
            "mp3"
        );
    }

    #[test]
    fn m4a_uses_aac() {
        let params = JobParams {
            audio_format: AudioFormat::M4a,
            audio_bitrate_k: 256,
            ..Default::default()
        };
        let stages =
            ExtractAudio.build_stages("in.mp4", "out.m4a", Path::new("/wd"), &probe(), &params);
        let a = &stages[0].args;
        assert!(a.windows(2).any(|w| w == ["-c:a", "aac"]));
        assert!(a.windows(2).any(|w| w == ["-b:a", "256k"]));
        assert_eq!(ExtractAudio.output_ext("in.m4a", &params), "m4a");
    }
}
