//! Inspecting a dropped file with `ffprobe`.
//!
//! The subprocess call ([`probe`]) is kept thin; all the interesting logic lives
//! in [`parse_probe_json`], which is pure and unit-tested against captured
//! `ffprobe` output so we don't need a real file (or ffmpeg) to test it.

use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// What we learn about a dropped file. Everything the UI and the ops need.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeResult {
    /// A *moving* picture: a real (non-cover-art) video stream with a duration.
    pub is_video: bool,
    /// A *still* image: a video stream with no duration (png/jpg/webp/heic…).
    pub is_image: bool,
    pub duration_s: f64,
    pub width: u32,
    pub height: u32,
    pub video_codec: String,
    pub has_audio: bool,
    pub audio_codec: String,
}

impl ProbeResult {
    /// Nothing we can work with (audio-only, document, unreadable, …).
    fn none() -> Self {
        ProbeResult {
            is_video: false,
            is_image: false,
            duration_s: 0.0,
            width: 0,
            height: 0,
            video_codec: String::new(),
            has_audio: false,
            audio_codec: String::new(),
        }
    }
}

/// Fast pre-check used before spawning ffprobe: obvious non-video extensions
/// short-circuit to "not a video" without a subprocess. A match here is *not*
/// authoritative (ffprobe still runs) — it only rejects the clearly-wrong.
pub fn extension_looks_like_video(path: &str) -> bool {
    const VIDEO_EXTS: &[&str] = &[
        "mp4", "mov", "m4v", "webm", "mkv", "avi", "flv", "wmv", "mpg", "mpeg", "ts", "m2ts",
        "3gp", "ogv",
    ];
    ext_matches(path, VIDEO_EXTS)
}

pub fn extension_looks_like_image(path: &str) -> bool {
    const IMAGE_EXTS: &[&str] = &[
        "jpg", "jpeg", "png", "webp", "heic", "heif", "gif", "bmp", "tiff", "tif", "tga",
    ];
    ext_matches(path, IMAGE_EXTS)
}

/// Worth spawning ffprobe? Video or image extension, or no extension at all.
/// A clearly-unrelated extension (.csv, .zip, …) is rejected without a probe.
pub fn extension_maybe_media(path: &str) -> bool {
    match Path::new(path).extension() {
        None => true, // no extension → let ffprobe decide
        Some(_) => extension_looks_like_video(path) || extension_looks_like_image(path),
    }
}

fn ext_matches(path: &str, exts: &[&str]) -> bool {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some(ext) => exts.contains(&ext.to_ascii_lowercase().as_str()),
        None => true, // no extension → let ffprobe decide (don't reject)
    }
}

/// Run `ffprobe` on `path` and interpret the result.
///
/// `ffprobe_bin` is the path/name of the ffprobe binary (system `ffprobe`
/// during dev, the bundled one in the shipped app).
pub fn probe(ffprobe_bin: &str, path: &str) -> ProbeResult {
    let output = Command::new(ffprobe_bin)
        .args([
            "-v",
            "quiet",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            path,
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let json = String::from_utf8_lossy(&out.stdout);
            parse_probe_json(&json)
        }
        _ => ProbeResult::none(),
    }
}

// ---- pure parsing over the ffprobe JSON shape --------------------------------

#[derive(Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<Stream>,
    #[serde(default)]
    format: Option<Format>,
}

#[derive(Deserialize)]
struct Stream {
    #[serde(default)]
    codec_type: String,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    disposition: Option<Disposition>,
}

#[derive(Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: i32,
}

#[derive(Deserialize)]
struct Format {
    #[serde(default)]
    duration: Option<String>,
}

/// Interpret raw ffprobe JSON into a [`ProbeResult`]. Pure — no I/O.
pub fn parse_probe_json(json: &str) -> ProbeResult {
    let parsed: FfprobeOutput = match serde_json::from_str(json) {
        Ok(p) => p,
        Err(_) => return ProbeResult::none(),
    };

    // A genuine picture stream: codec_type == "video" and NOT an attached
    // picture (album art). Covers both moving video and still images.
    let video_stream = parsed.streams.iter().find(|s| {
        s.codec_type == "video" && s.disposition.as_ref().map(|d| d.attached_pic).unwrap_or(0) == 0
    });

    let Some(vs) = video_stream else {
        return ProbeResult::none();
    };

    let audio_stream = parsed.streams.iter().find(|s| s.codec_type == "audio");

    // Prefer the container (format) duration; fall back to the video stream's.
    let duration_s = parsed
        .format
        .as_ref()
        .and_then(|f| f.duration.as_ref())
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| vs.duration.as_ref().and_then(|d| d.parse::<f64>().ok()))
        .unwrap_or(0.0);

    // With a real picture stream: a duration means video; no duration means a
    // still image (png/jpg/webp/heic all report no duration).
    let is_video = duration_s > 0.0;

    ProbeResult {
        is_video,
        is_image: !is_video,
        duration_s,
        width: vs.width.unwrap_or(0),
        height: vs.height.unwrap_or(0),
        video_codec: vs.codec_name.clone(),
        has_audio: audio_stream.is_some(),
        audio_codec: audio_stream
            .map(|a| a.codec_name.clone())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_normal_video() {
        let json = r#"{
            "streams": [
                {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"duration":"12.5"},
                {"codec_type":"audio","codec_name":"aac"}
            ],
            "format": {"duration":"12.520000"}
        }"#;
        let r = parse_probe_json(json);
        assert!(r.is_video);
        assert_eq!(r.width, 1920);
        assert_eq!(r.height, 1080);
        assert_eq!(r.video_codec, "h264");
        assert!(r.has_audio);
        assert_eq!(r.audio_codec, "aac");
        assert!((r.duration_s - 12.52).abs() < 0.001);
    }

    #[test]
    fn png_is_an_image_not_a_video() {
        // A still image: a real video stream, but no duration.
        let json = r#"{
            "streams": [{"codec_type":"video","codec_name":"png","width":4000,"height":3000}],
            "format": {"format_name":"png_pipe"}
        }"#;
        let r = parse_probe_json(json);
        assert!(r.is_image);
        assert!(!r.is_video);
        assert_eq!(r.width, 4000);
        assert_eq!(r.video_codec, "png");
    }

    #[test]
    fn mp3_cover_art_is_neither_video_nor_image() {
        let json = r#"{
            "streams": [
                {"codec_type":"audio","codec_name":"mp3"},
                {"codec_type":"video","codec_name":"mjpeg","width":300,"height":300,"disposition":{"attached_pic":1}}
            ]
        }"#;
        let r = parse_probe_json(json);
        assert!(!r.is_video);
        assert!(!r.is_image);
    }

    #[test]
    fn image_extension_precheck() {
        assert!(extension_looks_like_image("/x/photo.JPG"));
        assert!(extension_looks_like_image("/x/art.webp"));
        assert!(!extension_looks_like_image("/x/clip.mp4"));
        assert!(extension_maybe_media("/x/photo.png"));
        assert!(extension_maybe_media("/x/clip.mov"));
        assert!(!extension_maybe_media("/x/data.csv"));
    }

    #[test]
    fn video_with_no_audio() {
        let json = r#"{
            "streams": [{"codec_type":"video","codec_name":"vp9","width":640,"height":480,"duration":"3.0"}],
            "format": {"duration":"3.0"}
        }"#;
        let r = parse_probe_json(json);
        assert!(r.is_video);
        assert!(!r.has_audio);
        assert_eq!(r.video_codec, "vp9");
    }

    #[test]
    fn mp3_with_cover_art_is_not_video() {
        // Album art shows up as a video stream flagged attached_pic — not a video.
        let json = r#"{
            "streams": [
                {"codec_type":"audio","codec_name":"mp3"},
                {"codec_type":"video","codec_name":"mjpeg","width":300,"height":300,"disposition":{"attached_pic":1}}
            ],
            "format": {"duration":"180.0"}
        }"#;
        let r = parse_probe_json(json);
        assert!(!r.is_video);
    }

    #[test]
    fn garbage_json_is_not_video() {
        assert!(!parse_probe_json("not json at all").is_video);
        assert!(!parse_probe_json("{}").is_video);
    }

    #[test]
    fn extension_precheck() {
        assert!(extension_looks_like_video("/x/clip.mp4"));
        assert!(extension_looks_like_video("/x/clip.WEBM")); // case-insensitive
        assert!(!extension_looks_like_video("/x/photo.png"));
        assert!(!extension_looks_like_video("/x/data.csv"));
        assert!(extension_looks_like_video("/x/noext")); // unknown → let ffprobe decide
    }
}
