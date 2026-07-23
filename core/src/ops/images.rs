//! Image operations: convert, resize, compress.
//!
//! macOS `sips` is the workhorse: it reads everything (incl. tiled HEIC, which
//! ffmpeg mangles) and preserves EXIF orientation. ffmpeg is used only to write
//! WebP (which sips can't) — and only from a JPEG source, because ffmpeg *bakes*
//! JPEG EXIF orientation into its output (it ignores PNG orientation, which was
//! the earlier rotation bug).

use super::{base_args, ext_of, progress_args, ImageFormat, JobParams, Op, OpId, Stage};
use crate::probe::ProbeResult;
use std::path::Path;

/// Formats ffmpeg can't decode correctly — must go through sips.
fn needs_native_decode(input: &str) -> bool {
    matches!(ext_of(input).as_str(), "heic" | "heif" | "avif")
}

/// sips's format name for an output extension.
fn sips_format(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "jpeg",
        "png" => "png",
        "gif" => "gif",
        "tif" | "tiff" => "tiff",
        _ => "jpeg",
    }
}

/// Output extension for ops that keep the source format. HEIC/HEIF/AVIF can't
/// be re-written in place, so those become JPG.
fn keep_ext(input: &str) -> String {
    match ext_of(input).as_str() {
        "" | "heic" | "heif" | "avif" => "jpg".into(),
        other => other.to_string(),
    }
}

/// A sips convert/compress stage: `-s format <fmt> [-s formatOptions <q>] in --out out`.
fn sips_encode(input: &str, output: &str, ext: &str, quality: u32) -> Stage {
    let mut args = vec!["-s".into(), "format".into(), sips_format(ext).into()];
    if matches!(ext, "jpg" | "jpeg") {
        args.extend([
            "-s".into(),
            "formatOptions".into(),
            quality.clamp(1, 100).to_string(),
        ]);
    }
    args.push(input.into());
    args.extend(["--out".into(), output.into()]);
    Stage::sips(args, 1.0)
}

/// For the WebP path (ffmpeg only): if the source is HEIC, decode it to a
/// JPEG in `workdir` (orientation preserved as an EXIF tag ffmpeg will bake);
/// otherwise ffmpeg reads the source directly.
fn webp_source(input: &str, workdir: &Path) -> (Vec<Stage>, String) {
    if needs_native_decode(input) {
        let decoded = workdir.join("decoded.jpg").to_string_lossy().into_owned();
        let sips = Stage::sips(
            vec![
                "-s".into(),
                "format".into(),
                "jpeg".into(),
                "-s".into(),
                "formatOptions".into(),
                "100".into(),
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

fn webp_args(ff_input: &str, output: &str, quality: u32, extra_vf: Option<String>) -> Vec<String> {
    let mut args = base_args(ff_input);
    args.extend(progress_args());
    if let Some(vf) = extra_vf {
        args.push("-vf".into());
        args.push(vf);
    }
    args.extend([
        "-c:v".into(),
        "libwebp".into(),
        "-quality".into(),
        quality.clamp(1, 100).to_string(),
        "-frames:v".into(),
        "1".into(),
    ]);
    args.push(output.into());
    args
}

/// Downscale to fit within `max`×`max`, preserving aspect and never upscaling.
fn scale_filter(max: u32) -> String {
    format!("scale=min({max}\\,iw):min({max}\\,ih):force_original_aspect_ratio=decrease")
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
        match params.image_format {
            ImageFormat::Webp => {
                let (mut stages, ff_input) = webp_source(input, workdir);
                let last = stages.last().map(|_| 0.6).unwrap_or(1.0);
                stages.push(Stage::ffmpeg(
                    webp_args(&ff_input, output, params.image_quality, None),
                    last,
                ));
                stages
            }
            ImageFormat::Jpg => vec![sips_encode(input, output, "jpg", params.image_quality)],
            ImageFormat::Png => vec![sips_encode(input, output, "png", params.image_quality)],
        }
    }
}

// MARK: Resize

pub struct ImageResize;

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
        if out_ext == "webp" {
            // Source is a (non-HEIC) webp; ffmpeg reads it fine.
            return vec![Stage::ffmpeg(
                webp_args(input, output, 90, Some(scale_filter(params.image_max_dim))),
                1.0,
            )];
        }
        // sips resizes and keeps orientation. `-Z` fits within max, aspect kept.
        let args = vec![
            "-Z".into(),
            params.image_max_dim.to_string(),
            "-s".into(),
            "format".into(),
            sips_format(&out_ext).into(),
            input.into(),
            "--out".into(),
            output.into(),
        ];
        vec![Stage::sips(args, 1.0)]
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
        if out_ext == "webp" {
            return vec![Stage::ffmpeg(
                webp_args(input, output, params.image_quality, None),
                1.0,
            )];
        }
        vec![sips_encode(input, output, &out_ext, params.image_quality)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::Tool;

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
    fn convert_to_jpg_uses_sips() {
        let p = JobParams::default(); // jpg, q80
        assert_eq!(ImageConvert.output_ext("photo.png", &p), "jpg");
        let stages =
            ImageConvert.build_stages("photo.png", "photo.jpg", Path::new("/wd"), &img_probe(), &p);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].tool, Tool::Sips);
        let a = &stages[0].args;
        assert!(a.windows(2).any(|w| w == ["format", "jpeg"]));
        assert!(a.windows(2).any(|w| w == ["formatOptions", "80"]));
        assert_eq!(a.last().unwrap(), "photo.jpg");
    }

    #[test]
    fn convert_heic_to_jpg_is_sips_only() {
        // HEIC → JPG needs no ffmpeg: sips reads HEIC and keeps orientation.
        let p = JobParams::default();
        let stages =
            ImageConvert.build_stages("IMG.HEIC", "IMG.jpg", Path::new("/wd"), &img_probe(), &p);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].tool, Tool::Sips);
    }

    #[test]
    fn convert_heic_to_webp_decodes_via_sips_jpeg_then_ffmpeg() {
        let p = JobParams {
            image_format: ImageFormat::Webp,
            ..Default::default()
        };
        let stages =
            ImageConvert.build_stages("IMG.HEIC", "IMG.webp", Path::new("/wd"), &img_probe(), &p);
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].tool, Tool::Sips);
        // Decode to JPEG (so ffmpeg bakes orientation), not PNG.
        assert!(stages[0].args.iter().any(|s| s == "/wd/decoded.jpg"));
        assert_eq!(stages[1].tool, Tool::Ffmpeg);
        assert!(stages[1].args.iter().any(|s| s == "/wd/decoded.jpg"));
        assert!(stages[1].args.windows(2).any(|w| w == ["-c:v", "libwebp"]));
        assert_eq!(stages[1].args.last().unwrap(), "IMG.webp");
    }

    #[test]
    fn convert_jpg_to_webp_is_ffmpeg_direct() {
        let p = JobParams {
            image_format: ImageFormat::Webp,
            ..Default::default()
        };
        let stages =
            ImageConvert.build_stages("pic.jpg", "pic.webp", Path::new("/wd"), &img_probe(), &p);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].tool, Tool::Ffmpeg);
    }

    #[test]
    fn resize_uses_sips_z_and_maps_heic_to_jpg() {
        let p = JobParams {
            image_max_dim: 1280,
            ..Default::default()
        };
        assert_eq!(ImageResize.output_ext("IMG.HEIC", &p), "jpg");
        let stages = ImageResize.build_stages(
            "IMG.HEIC",
            "IMG-resized.jpg",
            Path::new("/wd"),
            &img_probe(),
            &p,
        );
        assert_eq!(stages[0].tool, Tool::Sips);
        let a = &stages[0].args;
        assert!(a.windows(2).any(|w| w == ["-Z", "1280"]));
        assert!(a.windows(2).any(|w| w == ["format", "jpeg"]));
    }

    #[test]
    fn compress_jpg_uses_sips_quality() {
        let p = JobParams {
            image_quality: 60,
            ..Default::default()
        };
        let stages = ImageCompress.build_stages(
            "pic.jpg",
            "pic-compressed.jpg",
            Path::new("/wd"),
            &img_probe(),
            &p,
        );
        assert_eq!(stages[0].tool, Tool::Sips);
        assert!(stages[0]
            .args
            .windows(2)
            .any(|w| w == ["formatOptions", "60"]));
    }
}
