//! The Swift↔Rust bridge (swift-bridge).
//!
//! Swift calls into the `extern "Rust"` functions; the shared structs cross the
//! boundary by value. Job execution + progress callbacks are added on top of
//! this in M3. Everything here delegates to the plain-Rust engine in the rest
//! of the crate.

use crate::ops::{op_for, AudioFormat, CompressMode, JobParams, OpId, VideoCodec};
use crate::{menu_for, probe};
use std::sync::OnceLock;

#[swift_bridge::bridge]
mod ffi {
    // Result of inspecting a dropped file — mirrors crate::probe::ProbeResult.
    #[swift_bridge(swift_repr = "struct")]
    struct ProbeInfo {
        is_video: bool,
        duration_s: f64,
        width: u32,
        height: u32,
        has_audio: bool,
        video_codec: String,
        audio_codec: String,
    }

    // A point-in-time view of a running/finished job.
    // status: 0=running 1=done 2=error 3=cancelled.
    #[swift_bridge(swift_repr = "struct")]
    struct ProgressSnapshot {
        pct: f32,
        eta_s: f64,
        status: u32,
        output_path: String,
        error: String,
    }

    // Advanced knobs from the UI. Enums are passed as small u32 codes;
    // max_height 0 means "keep original".
    #[swift_bridge(swift_repr = "struct")]
    struct JobParamsFFI {
        video_codec: u32,   // 0 h264, 1 hevc
        crf: u32,
        max_height: u32,    // 0 = original
        hw_accel: bool,
        compress_mode: u32, // 0 target-size, 1 crf
        target_mb: f64,
        audio_format: u32,  // 0 mp3, 1 m4a
        audio_bitrate_k: u32,
        gif_fps: u32,
        gif_width: u32,
    }

    extern "Rust" {
        // Call once at launch with the paths to the ffmpeg/ffprobe binaries
        // (bundled in the shipped app; system tools during dev).
        fn init_engine(ffmpeg_path: String, ffprobe_path: String);

        // Inspect a file. Runs a fast extension check, then ffprobe.
        fn probe_file(path: String) -> ProbeInfo;

        // Op ids of the preset menu for a video, in display order.
        // (swift-bridge can't return a Vec of shared structs, so the UI pairs
        //  these ids with op_label() to build its buttons.)
        fn menu_op_ids() -> Vec<u32>;

        // Human-readable label for an op id (empty string if unknown).
        fn op_label(op_id: u32) -> String;

        // Start an operation on a background thread; returns a job id.
        fn start_job(path: String, op_id: u32, params: JobParamsFFI) -> u64;

        // Non-blocking snapshot of a job's progress/outcome. Poll on a timer.
        fn poll_job(job_id: u64) -> ProgressSnapshot;

        // Request cancellation of a running job.
        fn cancel_job(job_id: u64);

        // Release a finished job's bookkeeping.
        fn release_job(job_id: u64);
    }
}

// ---- engine configuration ----------------------------------------------------

struct EngineCfg {
    ffmpeg: String,
    ffprobe: String,
}

static CFG: OnceLock<EngineCfg> = OnceLock::new();

fn init_engine(ffmpeg_path: String, ffprobe_path: String) {
    // Set once at launch; ignore a second call.
    let _ = CFG.set(EngineCfg {
        ffmpeg: ffmpeg_path,
        ffprobe: ffprobe_path,
    });
}

/// ffprobe path, falling back to system `ffprobe` if `init_engine` wasn't called.
pub(crate) fn ffprobe_bin() -> String {
    CFG.get()
        .map(|c| c.ffprobe.clone())
        .unwrap_or_else(|| "ffprobe".into())
}

/// ffmpeg path, falling back to system `ffmpeg`.
pub(crate) fn ffmpeg_bin() -> String {
    CFG.get()
        .map(|c| c.ffmpeg.clone())
        .unwrap_or_else(|| "ffmpeg".into())
}

// ---- bridged functions -------------------------------------------------------

fn probe_file(path: String) -> ffi::ProbeInfo {
    // Obvious non-videos (e.g. .png, .csv) never spawn ffprobe.
    if !probe::extension_looks_like_video(&path) {
        return not_video();
    }
    let p = probe::probe(&ffprobe_bin(), &path);
    ffi::ProbeInfo {
        is_video: p.is_video,
        duration_s: p.duration_s,
        width: p.width,
        height: p.height,
        has_audio: p.has_audio,
        video_codec: p.video_codec,
        audio_codec: p.audio_codec,
    }
}

fn not_video() -> ffi::ProbeInfo {
    ffi::ProbeInfo {
        is_video: false,
        duration_s: 0.0,
        width: 0,
        height: 0,
        has_audio: false,
        video_codec: String::new(),
        audio_codec: String::new(),
    }
}

fn menu_op_ids() -> Vec<u32> {
    // The menu is the same set for any video; pass a minimal video probe to get
    // the canonical ordered op ids from the registry.
    let dummy = probe::ProbeResult {
        is_video: true,
        duration_s: 0.0,
        width: 0,
        height: 0,
        has_audio: true,
        video_codec: String::new(),
        audio_codec: String::new(),
    };
    menu_for(&dummy).into_iter().map(|m| m.op_id).collect()
}

fn op_label(op_id: u32) -> String {
    OpId::from_u32(op_id)
        .map(|id| op_for(id).label().to_string())
        .unwrap_or_default()
}

fn start_job(path: String, op_id: u32, params: ffi::JobParamsFFI) -> u64 {
    crate::jobs::start(ffmpeg_bin(), ffprobe_bin(), path, op_id, to_job_params(params))
}

fn to_job_params(p: ffi::JobParamsFFI) -> JobParams {
    JobParams {
        video_codec: if p.video_codec == 1 { VideoCodec::Hevc } else { VideoCodec::H264 },
        crf: p.crf.clamp(0, 51) as u8,
        max_height: if p.max_height == 0 { None } else { Some(p.max_height) },
        hw_accel: p.hw_accel,
        compress_mode: if p.compress_mode == 1 {
            CompressMode::Crf
        } else {
            CompressMode::TargetSize
        },
        target_mb: p.target_mb,
        audio_format: if p.audio_format == 1 { AudioFormat::M4a } else { AudioFormat::Mp3 },
        audio_bitrate_k: p.audio_bitrate_k,
        gif_fps: p.gif_fps,
        gif_width: p.gif_width,
    }
}

fn poll_job(job_id: u64) -> ffi::ProgressSnapshot {
    let s = crate::jobs::poll(job_id);
    ffi::ProgressSnapshot {
        pct: s.pct,
        eta_s: s.eta_s,
        status: s.status,
        output_path: s.output_path,
        error: s.error,
    }
}

fn cancel_job(job_id: u64) {
    crate::jobs::cancel(job_id);
}

fn release_job(job_id: u64) {
    crate::jobs::release(job_id);
}
