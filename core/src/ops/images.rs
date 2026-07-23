//! Image operations: convert, resize, compress.
//!
//! Mostly ffmpeg-native (ffmpeg reads/writes stills). The exception is tiled
//! HEIC/HEIF/AVIF, which ffmpeg decodes incorrectly (it returns a single tile —
//! a small crop). For those we decode to a full-resolution PNG with macOS
//! `sips` first, then run the normal ffmpeg op on that.

use super::{base_args, ext_of, progress_args, JobParams, Op, OpId, Stage};
use crate::probe::ProbeResult;
use std::path::Path;

/// Formats ffmpeg mishandles — decode natively with sips first.
fn needs_native_decode(input: &str) -> bool {
    matches!(ext_of(input).as_str(), "heic" | "heif" | "avif")
}

/// If the input needs native decoding, returns a leading sips stage that writes
/// a full-res PNG into `workdir`, plus the path the ffmpeg stage should read.
/// Otherwise no extra stage and the original input.
fn decode_prefix(input: &str, workdir: &Path) -> (Vec<Stage>, String) {
    if needs_native_decode(input) {
        let decoded = workdir.join("decoded.png").to_string_lossy().into_owned();
        let sips = Stage::sips(
            vec![
                "-s".into(),
                "format".into(),
                "png".into(),
                input.into(),
                "--out".into(),
                decoded.clone(),
            ],
            0.4,
        );
        (vec![sips], decoded)
    } else {
        (Vec::new(), input.to_string())
    }
}

/// Map a 1–100 quality to an mjpeg `-q:v` qscale (2 = best … 31 = worst).
fn jpg_qscale(quality: u32) -> String {
    let q = quality.clamp(1, 100) as f64;
    let scale = 2.0 + (100.0 - q) * 29.0 / 100.0;
    ((scale.round() as u32).clamp(2, 31)).to_string()
}

/// Append the encoder args for a given output extension + quality.
fn encode_args(out_ext: &str, quality: u32, args: &mut Vec<String>) {
    match out_ext {
        "jpg" | "jpeg" => args.extend(["-q:v".into(), jpg_qscale(quality)]),
        "webp" => args.extend([
            "-c:v".into(),
            "libwebp".into(),
            "-quality".into(),
            quality.clamp(1, 100).to_string(),
        ]),
        _ => {} // png / other: lossless, nothing to tune
    }
}

/// Output extension for ops that keep the source format. HEIC/HEIF/AVIF can't
/// be written by ffmpeg, so those become JPG.
fn keep_ext(input: &str) -> String {
    match ext_of(input).as_str() {
        "" => "jpg".into(),
        "heic" | "heif" | "avif" => "jpg".into(),
        other => other.to_string(),
    }
}

/// Weight for the final ffmpeg stage given whether a decode stage precedes it.
fn ff_weight(prefix: &[Stage]) -> f32 {
    if prefix.is_empty() {
        1.0
    } else {
        0.6
    }
}

// MARK: Convert

pub struct ImageConvert;

impl Op for ImageConvert {
    fn id(&self) -> OpId {
        OpId::ImageConvert
    }
    fn label(&self) -> &'static str {
        "Convert"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        String::new()
    }
    fn output_ext(&self, _input: &str, params: &JobParams) -> String {
        params.image_format.ext().to_string()
    }

    fn build_stages(
        &self,
        input: &str,
        output: &str,
        workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let out_ext = params.image_format.ext();
        let (mut stages, ff_input) = decode_prefix(input, workdir);
        let mut args = base_args(&ff_input);
        args.extend(progress_args());
        encode_args(out_ext, params.image_quality, &mut args);
        args.extend(["-frames:v".into(), "1".into()]);
        args.push(output.into());
        let w = ff_weight(&stages);
        stages.push(Stage::ffmpeg(args, w));
        stages
    }
}

// MARK: Resize

pub struct ImageResize;

/// Downscale to fit within `max`×`max`, preserving aspect and never upscaling.
fn scale_filter(max: u32) -> String {
    format!("scale=min({max}\\,iw):min({max}\\,ih):force_original_aspect_ratio=decrease")
}

impl Op for ImageResize {
    fn id(&self) -> OpId {
        OpId::ImageResize
    }
    fn label(&self) -> &'static str {
        "Resize"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        "resized".into()
    }
    fn output_ext(&self, input: &str, _params: &JobParams) -> String {
        keep_ext(input)
    }

    fn build_stages(
        &self,
        input: &str,
        output: &str,
        workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let out_ext = keep_ext(input);
        let (mut stages, ff_input) = decode_prefix(input, workdir);
        let mut args = base_args(&ff_input);
        args.extend(progress_args());
        args.push("-vf".into());
        args.push(scale_filter(params.image_max_dim));
        encode_args(&out_ext, 95, &mut args); // keep quality high on resize
        args.extend(["-frames:v".into(), "1".into()]);
        args.push(output.into());
        let w = ff_weight(&stages);
        stages.push(Stage::ffmpeg(args, w));
        stages
    }
}

// MARK: Compress

pub struct ImageCompress;

impl Op for ImageCompress {
    fn id(&self) -> OpId {
        OpId::ImageCompress
    }
    fn label(&self) -> &'static str {
        "Compress"
    }
    fn output_suffix(&self, _params: &JobParams) -> String {
        "compressed".into()
    }
    fn output_ext(&self, input: &str, _params: &JobParams) -> String {
        keep_ext(input)
    }

    fn build_stages(
        &self,
        input: &str,
        output: &str,
        workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let out_ext = keep_ext(input);
        let (mut stages, ff_input) = decode_prefix(input, workdir);
        let mut args = base_args(&ff_input);
        args.extend(progress_args());
        encode_args(&out_ext, params.image_quality, &mut args);
        args.extend(["-frames:v".into(), "1".into()]);
        args.push(output.into());
        let w = ff_weight(&stages);
        stages.push(Stage::ffmpeg(args, w));
        stages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{ImageFormat, Tool};

    fn img_probe() -> ProbeResult {
        ProbeResult {
            is_video: false,
            is_image: true,
            duration_s: 0.0,
            width: 4000,
            height: 3000,
            video_codec: "png".into(),
            has_audio: false,
            audio_codec: String::new(),
        }
    }

    #[test]
    fn convert_defaults_to_jpg() {
        let p = JobParams::default();
        assert_eq!(ImageConvert.output_ext("photo.png", &p), "jpg");
        let stages =
            ImageConvert.build_stages("photo.png", "photo.jpg", Path::new("/wd"), &img_probe(), &p);
        assert_eq!(stages.len(), 1); // non-heic → no decode stage
        let a = &stages[0].args;
        assert!(a.windows(2).any(|w| w == ["-q:v", jpg_qscale(80).as_str()]));
        assert!(a.windows(2).any(|w| w == ["-frames:v", "1"]));
        assert_eq!(a.last().unwrap(), "photo.jpg");
    }

    #[test]
    fn convert_to_webp() {
        let p = JobParams {
            image_format: ImageFormat::Webp,
            image_quality: 70,
            ..Default::default()
        };
        assert_eq!(ImageConvert.output_ext("photo.png", &p), "webp");
        let stages = ImageConvert.build_stages(
            "photo.png",
            "photo.webp",
            Path::new("/wd"),
            &img_probe(),
            &p,
        );
        let a = &stages[0].args;
        assert!(a.windows(2).any(|w| w == ["-c:v", "libwebp"]));
        assert!(a.windows(2).any(|w| w == ["-quality", "70"]));
    }

    #[test]
    fn heic_gets_a_sips_decode_stage_then_ffmpeg() {
        let p = JobParams::default();
        // HEIC can't keep its format for resize/compress → becomes jpg.
        assert_eq!(ImageResize.output_ext("IMG.HEIC", &p), "jpg");
        let stages =
            ImageConvert.build_stages("IMG.HEIC", "IMG.jpg", Path::new("/wd"), &img_probe(), &p);
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].tool, Tool::Sips);
        assert!(stages[0].args.iter().any(|s| s == "/wd/decoded.png"));
        assert_eq!(stages[1].tool, Tool::Ffmpeg);
        // ffmpeg stage reads the decoded PNG, not the HEIC.
        assert!(stages[1].args.iter().any(|s| s == "/wd/decoded.png"));
        assert_eq!(stages[1].args.last().unwrap(), "IMG.jpg");
    }

    #[test]
    fn resize_keeps_ext_and_caps_dimension() {
        let p = JobParams {
            image_max_dim: 1280,
            ..Default::default()
        };
        assert_eq!(ImageResize.output_ext("shot.jpeg", &p), "jpeg");
        let stages = ImageResize.build_stages(
            "shot.jpeg",
            "shot-resized.jpeg",
            Path::new("/wd"),
            &img_probe(),
            &p,
        );
        let a = &stages[0].args;
        assert!(a
            .iter()
            .any(|s| s.contains("force_original_aspect_ratio=decrease") && s.contains("1280")));
    }

    #[test]
    fn compress_keeps_format() {
        let p = JobParams::default();
        assert_eq!(ImageCompress.output_ext("pic.jpg", &p), "jpg");
        let stages = ImageCompress.build_stages(
            "pic.jpg",
            "pic-compressed.jpg",
            Path::new("/wd"),
            &img_probe(),
            &p,
        );
        assert!(stages[0].args.contains(&"-q:v".to_string()));
    }
}
