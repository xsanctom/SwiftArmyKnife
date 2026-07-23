//! Turn a clip into an optimized GIF.
//!
//! Two stages: generate an optimal 256-colour palette, then render the GIF
//! using it. A shared palette produces far better quality than ffmpeg's
//! default on-the-fly quantisation.

use super::{base_args, progress_args, JobParams, Op, OpId, Stage};
use crate::probe::ProbeResult;
use std::path::Path;

pub struct Gif;

impl Op for Gif {
    fn id(&self) -> OpId {
        OpId::Gif
    }
    fn label(&self) -> &'static str {
        "Make GIF"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        String::new()
    }
    fn output_ext(&self, _input: &str, _params: &JobParams) -> String {
        "gif".into()
    }

    fn build_stages(
        &self,
        input: &str,
        output: &str,
        workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let fps = params.gif_fps;
        let width = params.gif_width;
        let palette = workdir.join("palette.png").to_string_lossy().into_owned();

        // Stage 1: palettegen → palette.png
        let mut p1 = base_args(input);
        p1.extend(progress_args());
        p1.push("-vf".into());
        p1.push(format!(
            "fps={fps},scale={width}:-1:flags=lanczos,palettegen"
        ));
        p1.push(palette.clone());

        // Stage 2: paletteuse (input + palette) → output.gif
        let mut p2 = base_args(input);
        p2.push("-i".into());
        p2.push(palette);
        p2.extend(progress_args());
        p2.push("-lavfi".into());
        p2.push(format!(
            "fps={fps},scale={width}:-1:flags=lanczos[x];[x][1:v]paletteuse"
        ));
        p2.extend(["-loop".into(), "0".into()]);
        p2.push(output.into());

        vec![Stage::ffmpeg(p1, 0.5), Stage::ffmpeg(p2, 0.5)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> ProbeResult {
        ProbeResult {
            is_video: true,
            is_image: false,
            duration_s: 5.0,
            width: 1280,
            height: 720,
            video_codec: "h264".into(),
            has_audio: false,
            audio_codec: String::new(),
        }
    }

    #[test]
    fn two_stages_palette_then_render() {
        let stages = Gif.build_stages(
            "in.mp4",
            "out.gif",
            Path::new("/wd"),
            &probe(),
            &JobParams::default(),
        );
        assert_eq!(stages.len(), 2);
        // Stage 1 generates the palette into the workdir.
        assert!(stages[0].args.iter().any(|s| s.contains("palettegen")));
        assert_eq!(stages[0].args.last().unwrap(), "/wd/palette.png");
        // Stage 2 consumes it and uses default fps/width.
        assert!(stages[1].args.iter().any(|s| s == "/wd/palette.png"));
        assert!(stages[1]
            .args
            .iter()
            .any(|s| s.contains("paletteuse") && s.contains("fps=12") && s.contains("scale=480")));
        assert_eq!(stages[1].args.last().unwrap(), "out.gif");
    }

    #[test]
    fn respects_custom_fps_and_width() {
        let params = JobParams {
            gif_fps: 24,
            gif_width: 320,
            ..Default::default()
        };
        let stages = Gif.build_stages("in.mp4", "out.gif", Path::new("/wd"), &probe(), &params);
        assert!(stages[0]
            .args
            .iter()
            .any(|s| s.contains("fps=24") && s.contains("scale=320")));
    }
}
