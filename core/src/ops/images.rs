//! Image operations: convert, resize, compress. All ffmpeg-native (ffmpeg
//! reads/writes stills), so they reuse the same engine as the video ops.

use super::{base_args, ext_of, progress_args, JobParams, Op, OpId, Stage};
use crate::probe::ProbeResult;
use std::path::Path;

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

/// Extension we keep for resize/compress (source ext, or jpg as a safe default).
fn keep_ext(input: &str) -> String {
    let e = ext_of(input);
    if e.is_empty() {
        "jpg".into()
    } else {
        e
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
        _workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let out_ext = params.image_format.ext();
        let mut args = base_args(input);
        args.extend(progress_args());
        encode_args(out_ext, params.image_quality, &mut args);
        args.extend(["-frames:v".into(), "1".into()]);
        args.push(output.into());
        vec![Stage { args, weight: 1.0 }]
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
        _workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let out_ext = keep_ext(input);
        let mut args = base_args(input);
        args.extend(progress_args());
        args.push("-vf".into());
        args.push(scale_filter(params.image_max_dim));
        encode_args(&out_ext, 95, &mut args); // keep quality high on resize
        args.extend(["-frames:v".into(), "1".into()]);
        args.push(output.into());
        vec![Stage { args, weight: 1.0 }]
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
        _workdir: &Path,
        _probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage> {
        let out_ext = keep_ext(input);
        let mut args = base_args(input);
        args.extend(progress_args());
        encode_args(&out_ext, params.image_quality, &mut args);
        args.extend(["-frames:v".into(), "1".into()]);
        args.push(output.into());
        vec![Stage { args, weight: 1.0 }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::ImageFormat;

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
        let a = &ImageConvert.build_stages(
            "photo.png",
            "photo.jpg",
            Path::new("/wd"),
            &img_probe(),
            &p,
        )[0]
        .args;
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
        let a = &ImageConvert.build_stages(
            "photo.png",
            "photo.webp",
            Path::new("/wd"),
            &img_probe(),
            &p,
        )[0]
        .args;
        assert!(a.windows(2).any(|w| w == ["-c:v", "libwebp"]));
        assert!(a.windows(2).any(|w| w == ["-quality", "70"]));
    }

    #[test]
    fn resize_keeps_ext_and_caps_dimension() {
        let p = JobParams {
            image_max_dim: 1280,
            ..Default::default()
        };
        assert_eq!(ImageResize.output_ext("shot.jpeg", &p), "jpeg");
        let a = &ImageResize.build_stages(
            "shot.jpeg",
            "shot-resized.jpeg",
            Path::new("/wd"),
            &img_probe(),
            &p,
        )[0]
        .args;
        assert!(a
            .iter()
            .any(|s| s.contains("force_original_aspect_ratio=decrease") && s.contains("1280")));
    }

    #[test]
    fn compress_keeps_format() {
        let p = JobParams::default();
        assert_eq!(ImageCompress.output_ext("pic.jpg", &p), "jpg");
        let a = &ImageCompress.build_stages(
            "pic.jpg",
            "pic-compressed.jpg",
            Path::new("/wd"),
            &img_probe(),
            &p,
        )[0]
        .args;
        assert!(a.contains(&"-q:v".to_string()));
    }
}
