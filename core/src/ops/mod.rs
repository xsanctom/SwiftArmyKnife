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

/// Stable numeric ids — these cross the FFI boundary as `u32`, so the values
/// must not change once the Swift side depends on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpId {
    Convert = 0,
    Compress = 1,
    ExtractAudio = 2,
    Gif = 3,
}

impl OpId {
    pub fn from_u32(v: u32) -> Option<OpId> {
        match v {
            0 => Some(OpId::Convert),
            1 => Some(OpId::Compress),
            2 => Some(OpId::ExtractAudio),
            3 => Some(OpId::Gif),
            _ => None,
        }
    }
}

/// One ffmpeg invocation within an operation. Most ops are a single stage;
/// GIF (palettegen → paletteuse) and target-size compress (two-pass) use two.
/// `weight` is this stage's share of overall progress and must sum to ~1.0
/// across an op's stages.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub args: Vec<String>,
    pub weight: f32,
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
        }
    }
}

/// An operation the app can perform on a video.
pub trait Op {
    fn id(&self) -> OpId;
    fn label(&self) -> &'static str;
    /// Suffix added to the output filename stem (may be empty).
    fn output_suffix(&self, params: &JobParams) -> String;
    /// Output extension without the dot (may depend on params, e.g. audio).
    fn output_ext(&self, params: &JobParams) -> String;
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
    }
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
    vec![
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ]
}
