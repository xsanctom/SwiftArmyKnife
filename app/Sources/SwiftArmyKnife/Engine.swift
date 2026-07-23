// Thin Swift-native wrapper over the generated swift-bridge FFI. Everything
// past this file deals in Swift types, never RustString/RustVec.

import Foundation

struct Probe {
    var isVideo: Bool
    var isImage: Bool
    var duration: Double
    var width: Int
    var height: Int
    var hasAudio: Bool
    var videoCodec: String
    var audioCodec: String

    var isMedia: Bool { isVideo || isImage }
}

struct Preset: Identifiable, Hashable {
    let id: UInt32 // op id
    let label: String
}

enum JobStatus {
    case running, done, error, cancelled
}

// Swift-native advanced knobs. Defaults match the Rust JobParams defaults, so
// a one-tap preset run just passes AdvancedParams().
struct AdvancedParams {
    var videoCodec: UInt32 = 0 // 0 h264, 1 hevc
    var crf: UInt32 = 20
    var maxHeight: UInt32 = 0 // 0 = original
    var hwAccel: Bool = false
    var compressMode: UInt32 = 0 // 0 target-size, 1 crf
    var targetMB: Double = 25
    var audioFormat: UInt32 = 0 // 0 mp3, 1 m4a
    var audioBitrateK: UInt32 = 192
    var gifFps: UInt32 = 12
    var gifWidth: UInt32 = 480
    var imageFormat: UInt32 = 0 // 0 jpg, 1 png, 2 webp
    var imageQuality: UInt32 = 80
    var imageMaxDim: UInt32 = 1920

    func toFFI() -> JobParamsFFI {
        JobParamsFFI(
            video_codec: videoCodec,
            crf: crf,
            max_height: maxHeight,
            hw_accel: hwAccel,
            compress_mode: compressMode,
            target_mb: targetMB,
            audio_format: audioFormat,
            audio_bitrate_k: audioBitrateK,
            gif_fps: gifFps,
            gif_width: gifWidth,
            image_format: imageFormat,
            image_quality: imageQuality,
            image_max_dim: imageMaxDim
        )
    }
}

// Swift-native snapshot. (Named to avoid clashing with the swift-bridge
// generated `ProgressSnapshot` shared struct.)
struct JobSnapshot {
    let pct: Double
    let etaSeconds: Double
    let status: JobStatus
    let outputPath: String
    let error: String
}

enum Engine {
    private(set) static var ffmpegPath = ""
    private(set) static var ffprobePath = ""

    /// Point the engine at the ffmpeg/ffprobe binaries. Call once at launch.
    static func initialize(ffmpeg: String, ffprobe: String) {
        ffmpegPath = ffmpeg
        ffprobePath = ffprobe
        init_engine(ffmpeg, ffprobe)
    }

    /// Both tools present and executable? (False → app can't do anything.)
    static var toolsReady: Bool {
        let fm = FileManager.default
        return fm.isExecutableFile(atPath: ffmpegPath) && fm.isExecutableFile(atPath: ffprobePath)
    }

    /// Suggested install command shown when tools are missing.
    static let installCommand = "brew install ffmpeg"

    static func probe(_ path: String) -> Probe {
        let info = probe_file(path)
        return Probe(
            isVideo: info.is_video,
            isImage: info.is_image,
            duration: info.duration_s,
            width: Int(info.width),
            height: Int(info.height),
            hasAudio: info.has_audio,
            videoCodec: info.video_codec.toString(),
            audioCodec: info.audio_codec.toString()
        )
    }

    /// Preset menu for the kinds present (union for a mixed batch).
    static func presets(video: Bool, image: Bool) -> [Preset] {
        menu_op_ids(video, image).map { Preset(id: $0, label: op_label($0).toString()) }
    }

    /// Fast extension-only classification: 0 = not media, 1 = video, 2 = image.
    static func classifyPath(_ path: String) -> Int {
        Int(classify_path(path))
    }

    /// Start a job on a Rust background thread; returns a job id to poll.
    static func startJob(path: String, opId: UInt32, params: AdvancedParams) -> UInt64 {
        start_job(path, opId, params.toFFI())
    }

    /// Cheap, non-blocking snapshot — safe to call from a main-thread timer.
    static func poll(_ jobId: UInt64) -> JobSnapshot {
        let s = poll_job(jobId)
        let status: JobStatus
        switch s.status {
        case 1: status = .done
        case 2: status = .error
        case 3: status = .cancelled
        default: status = .running
        }
        return JobSnapshot(
            pct: Double(s.pct),
            etaSeconds: s.eta_s,
            status: status,
            outputPath: s.output_path.toString(),
            error: s.error.toString()
        )
    }

    static func cancel(_ jobId: UInt64) { cancel_job(jobId) }
    static func release(_ jobId: UInt64) { release_job(jobId) }
}
