//! The operation registry.
//!
//! Each operation knows its label, the suffix + extension of its output, and
//! how to build the sequence of ffmpeg invocations ("stages") that carry it
//! out. Adding a future operation = add a file here and a match arm in
//! [`op_for`]. The set of ops is data the UI reads to build its preset menu.

use crate::probe::ProbeResult;
use std::path::Path;

mod audio;
mod compress;
mod convert;
mod gif;
mod images;

/// Stable numeric ids — these cross the FFI boundary as `u32`, so the values
/// must not change once the Swift side depends on them. Video ops are 0–3,
/// image ops are 10–12 (kept in separate ranges so the category is obvious).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpId {
    Convert = 0,
    Compress = 1,
    ExtractAudio = 2,
    Gif = 3,
    ImageConvert = 10,
    ImageResize = 11,
    ImageCompress = 12,
}

impl OpId {
    pub fn from_u32(v: u32) -> Option<OpId> {
        match v {
            0 => Some(OpId::Convert),
            1 => Some(OpId::Compress),
            2 => Some(OpId::ExtractAudio),
            3 => Some(OpId::Gif),
            10 => Some(OpId::ImageConvert),
            11 => Some(OpId::ImageResize),
            12 => Some(OpId::ImageCompress),
            _ => None,
        }
    }
}

/// Which external tool a stage invokes. Almost everything is ffmpeg; image ops
/// use a sips stage to decode formats ffmpeg mishandles (tiled HEIC/HEIF/AVIF).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    Sips,
}

/// One tool invocation within an operation. Most ops are a single stage;
/// GIF (palettegen → paletteuse) and target-size compress (two-pass) use two.
/// `weight` is this stage's share of overall progress and must sum to ~1.0
/// across an op's stages.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub tool: Tool,
    pub args: Vec<String>,
    pub weight: f32,
}

impl Stage {
    pub fn ffmpeg(args: Vec<String>, weight: f32) -> Stage {
        Stage {
            tool: Tool::Ffmpeg,
            args,
            weight,
        }
    }
    pub fn sips(args: Vec<String>, weight: f32) -> Stage {
        Stage {
            tool: Tool::Sips,
            args,
            weight,
        }
    }
}

// ---- tunable parameters (the "Advanced" knobs) -------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressMode {
    TargetSize,
    Crf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    M4a,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpg,
    Png,
    Webp,
}

impl ImageFormat {
    pub fn ext(self) -> &'static str {
        match self {
            ImageFormat::Jpg => "jpg",
            ImageFormat::Png => "png",
            ImageFormat::Webp => "webp",
        }
    }
}

/// All advanced knobs across all ops in one struct. Each op reads only the
/// fields it cares about; the defaults are the one-tap preset behaviour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JobParams {
    // Convert
    pub video_codec: VideoCodec,
    pub crf: u8,
    pub max_height: Option<u32>,
    pub hw_accel: bool,
    // Compress
    pub compress_mode: CompressMode,
    pub target_mb: f64,
    // Extract audio
    pub audio_format: AudioFormat,
    pub audio_bitrate_k: u32,
    // GIF
    pub gif_fps: u32,
    pub gif_width: u32,
    // Images
    pub image_format: ImageFormat,
    pub image_quality: u32, // 1–100 (jpg/webp)
    pub image_max_dim: u32, // longest side, px (resize)
}

impl Default for JobParams {
    fn default() -> Self {
        JobParams {
            video_codec: VideoCodec::H264,
            crf: 20,
            max_height: None,
            hw_accel: false,
            compress_mode: CompressMode::TargetSize,
            target_mb: 25.0,
            audio_format: AudioFormat::Mp3,
            audio_bitrate_k: 192,
            gif_fps: 12,
            gif_width: 480,
            image_format: ImageFormat::Jpg,
            image_quality: 80,
            image_max_dim: 1920,
        }
    }
}

/// An operation the app can perform on a video.
pub trait Op {
    fn id(&self) -> OpId;
    fn label(&self) -> &'static str;
    /// Suffix added to the output filename stem (may be empty).
    fn output_suffix(&self, params: &JobParams) -> String;
    /// Output extension without the dot. Takes the input path since some ops
    /// (image resize/compress) keep the source's extension.
    fn output_ext(&self, input: &str, params: &JobParams) -> String;
    /// Build the ordered ffmpeg invocations. `workdir` is a scratch dir the op
    /// may use for intermediate files (palette, two-pass log).
    fn build_stages(
        &self,
        input: &str,
        output: &str,
        workdir: &Path,
        probe: &ProbeResult,
        params: &JobParams,
    ) -> Vec<Stage>;
}

/// Look up an operation by id.
pub fn op_for(id: OpId) -> Box<dyn Op> {
    match id {
        OpId::Convert => Box::new(convert::Convert),
        OpId::Compress => Box::new(compress::Compress),
        OpId::ExtractAudio => Box::new(audio::ExtractAudio),
        OpId::Gif => Box::new(gif::Gif),
        OpId::ImageConvert => Box::new(images::ImageConvert),
        OpId::ImageResize => Box::new(images::ImageResize),
        OpId::ImageCompress => Box::new(images::ImageCompress),
    }
}

/// Lowercased extension of a path (no dot), or empty string.
pub(crate) fn ext_of(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Common leading args for every ffmpeg invocation: quiet banner, overwrite
/// output (we've already chosen a collision-free name), machine-readable
/// progress on stdout, and the input file.
fn base_args(input: &str) -> Vec<String> {
    vec![
        "-hide_banner".into(),
        "-nostdin".into(),
        "-y".into(),
        "-i".into(),
        input.into(),
    ]
}

/// Progress reporting flags appended to a stage that writes real output.
fn progress_args() -> Vec<String> {
    vec!["-progress".into(), "pipe:1".into(), "-nostats".into()]
}
