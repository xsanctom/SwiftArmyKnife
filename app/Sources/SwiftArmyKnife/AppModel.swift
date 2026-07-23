// App state machine + orchestration. Works on a list of files (a single drop
// is just a batch of one), running one job at a time with overall progress.

import AppKit
import SwiftUI

enum FileKind {
    case video, image, csv, xlsx
}

struct MediaFile {
    let path: String
    let kind: FileKind
}

struct PresetInfo {
    let presets: [Preset]
    let title: String
    let subtitle: String
    let symbol: String
    let count: Int
}

struct RunInfo {
    let label: String
    let index: Int // 1-based
    let total: Int
}

struct DoneInfo {
    let outputs: [String]
    let failed: Int
    let skipped: Int
}

@MainActor
final class AppModel: ObservableObject {
    enum Stage {
        case drop
        case probing
        case missingTools
        case unsupported
        case presets(PresetInfo)
        case running(RunInfo)
        case done(DoneInfo)
        case error(String)
    }

    @Published var stage: Stage = .drop
    @Published var progress: Double = 0 // overall 0...1
    @Published var etaSeconds: Double = 0

    private var files: [MediaFile] = []
    private var singleProbe: Probe?
    private var presetInfo: PresetInfo?

    private var batchTask: Task<Void, Never>?
    private var currentJobId: UInt64?
    private var cancelRequested = false

    // --- headless verification hooks (no effect in normal interactive use) ---
    private var autorunOpId: UInt32?
    private var resultFile: String?
    private var autoCancelMs: UInt64?
    private var autorunParams = AdvancedParams()

    func start() {
        let env = ProcessInfo.processInfo.environment
        var preloads: [String] = env["SAK_PRELOAD"].map { [$0] } ?? []
        autorunOpId = env["SAK_AUTORUN"].flatMap { UInt32($0) }
        resultFile = env["SAK_RESULT_FILE"]

        let args = CommandLine.arguments
        var i = 0
        while i < args.count {
            let next = i + 1 < args.count ? args[i + 1] : nil
            switch args[i] {
            case "--preload": if let n = next { preloads.append(n); i += 1 }
            case "--autorun": if let n = next { autorunOpId = UInt32(n); i += 1 }
            case "--result": if let n = next { resultFile = n; i += 1 }
            case "--autocancel": if let n = next { autoCancelMs = UInt64(n); i += 1 }
            case "--crf": if let n = next.flatMap({ UInt32($0) }) { autorunParams.crf = n; i += 1 }
            case "--codec": if let n = next.flatMap({ UInt32($0) }) { autorunParams.videoCodec = n; i += 1 }
            case "--maxh": if let n = next.flatMap({ UInt32($0) }) { autorunParams.maxHeight = n; i += 1 }
            case "--cmode": if let n = next.flatMap({ UInt32($0) }) { autorunParams.compressMode = n; i += 1 }
            case "--targetmb": if let n = next.flatMap({ Double($0) }) { autorunParams.targetMB = n; i += 1 }
            case "--audiofmt": if let n = next.flatMap({ UInt32($0) }) { autorunParams.audioFormat = n; i += 1 }
            case "--abitrate": if let n = next.flatMap({ UInt32($0) }) { autorunParams.audioBitrateK = n; i += 1 }
            case "--giffps": if let n = next.flatMap({ UInt32($0) }) { autorunParams.gifFps = n; i += 1 }
            case "--gifw": if let n = next.flatMap({ UInt32($0) }) { autorunParams.gifWidth = n; i += 1 }
            case "--imgfmt": if let n = next.flatMap({ UInt32($0) }) { autorunParams.imageFormat = n; i += 1 }
            case "--imgq": if let n = next.flatMap({ UInt32($0) }) { autorunParams.imageQuality = n; i += 1 }
            case "--imgmax": if let n = next.flatMap({ UInt32($0) }) { autorunParams.imageMaxDim = n; i += 1 }
            default: break
            }
            i += 1
        }

        if !Engine.toolsReady {
            stage = .missingTools
            return
        }
        let existing = preloads.filter { FileManager.default.fileExists(atPath: $0) }
        if !existing.isEmpty { handleDrop(existing) }
    }

    func recheckTools() {
        if Engine.toolsReady { stage = .drop }
    }

    func handleDrop(_ paths: [String]) {
        guard Engine.toolsReady else {
            stage = .missingTools
            return
        }
        stage = .probing
        Task {
            let gathered = await Task.detached { AppModel.gather(paths) }.value
            files = gathered.files
            singleProbe = gathered.single
            guard !files.isEmpty else {
                stage = .unsupported
                finishAutorun(ok: false, detail: "no media files")
                return
            }
            let info = buildPresetInfo()
            presetInfo = info
            stage = .presets(info)
            if let op = autorunOpId { run(opId: op, label: "auto", params: autorunParams) }
        }
    }

    // MARK: gather (folder expansion + categorisation, off-main)

    struct Gathered {
        let files: [MediaFile]
        let single: Probe?
    }

    nonisolated static func gather(_ paths: [String]) -> Gathered {
        let fm = FileManager.default
        var mediaPaths: [String] = []
        for p in paths {
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: p, isDirectory: &isDir) else { continue }
            if isDir.boolValue {
                // Expand a folder to the media files it contains (recursively).
                if let en = fm.enumerator(atPath: p) {
                    for case let rel as String in en {
                        let full = (p as NSString).appendingPathComponent(rel)
                        if Engine.classifyPath(full) != 0 { mediaPaths.append(full) }
                    }
                }
            } else {
                mediaPaths.append(p) // explicitly dropped file — kind decided below
            }
        }

        // A single file: spreadsheets are classified by extension; media get
        // the richer content-based probe (nice header, catches audio-only .webm).
        if mediaPaths.count == 1 {
            let p = mediaPaths[0]
            switch Engine.classifyPath(p) {
            case 3: return Gathered(files: [MediaFile(path: p, kind: .csv)], single: nil)
            case 4: return Gathered(files: [MediaFile(path: p, kind: .xlsx)], single: nil)
            default:
                let probe = Engine.probe(p)
                guard probe.isMedia else { return Gathered(files: [], single: nil) }
                let kind: FileKind = probe.isImage ? .image : .video
                return Gathered(files: [MediaFile(path: p, kind: kind)], single: probe)
            }
        }

        // A batch is categorised by extension — fast, no per-file subprocess.
        var files: [MediaFile] = []
        for p in mediaPaths {
            switch Engine.classifyPath(p) {
            case 1: files.append(MediaFile(path: p, kind: .video))
            case 2: files.append(MediaFile(path: p, kind: .image))
            case 3: files.append(MediaFile(path: p, kind: .csv))
            case 4: files.append(MediaFile(path: p, kind: .xlsx))
            default: break
            }
        }
        return Gathered(files: files, single: nil)
    }

    private func buildPresetInfo() -> PresetInfo {
        let videos = files.filter { $0.kind == .video }.count
        let images = files.filter { $0.kind == .image }.count
        let csvs = files.filter { $0.kind == .csv }.count
        let xlsxs = files.filter { $0.kind == .xlsx }.count
        let presets = Engine.presets(
            video: videos > 0, image: images > 0, csv: csvs > 0, xlsx: xlsxs > 0
        )

        if files.count == 1 {
            let kind = files[0].kind
            let subtitle: String
            switch kind {
            case .csv: subtitle = "CSV file"
            case .xlsx: subtitle = "Excel spreadsheet"
            default: subtitle = singleProbe.map(singleSummary) ?? ""
            }
            return PresetInfo(
                presets: presets,
                title: URL(fileURLWithPath: files[0].path).lastPathComponent,
                subtitle: subtitle,
                symbol: symbol(for: kind),
                count: 1
            )
        }
        var parts: [String] = []
        if videos > 0 { parts.append("\(videos) video\(videos == 1 ? "" : "s")") }
        if images > 0 { parts.append("\(images) image\(images == 1 ? "" : "s")") }
        let sheets = csvs + xlsxs
        if sheets > 0 { parts.append("\(sheets) spreadsheet\(sheets == 1 ? "" : "s")") }
        return PresetInfo(
            presets: presets,
            title: "\(files.count) files",
            subtitle: parts.joined(separator: " · "),
            symbol: "square.stack.3d.up.fill",
            count: files.count
        )
    }

    private func symbol(for kind: FileKind) -> String {
        switch kind {
        case .video: return "video.fill"
        case .image: return "photo.fill"
        case .csv, .xlsx: return "tablecells.fill"
        }
    }

    private func singleSummary(_ p: Probe) -> String {
        var parts: [String] = []
        if p.width > 0 { parts.append("\(p.width)×\(p.height)") }
        if p.duration > 0 { parts.append(formatDuration(p.duration)) }
        if !p.videoCodec.isEmpty { parts.append(p.videoCodec.uppercased()) }
        return parts.joined(separator: "  ·  ")
    }

    // MARK: run (sequential batch)

    func run(opId: UInt32, label: String, params: AdvancedParams = AdvancedParams()) {
        // Which files this op applies to (others in a mixed batch are skipped).
        let applies: (MediaFile) -> Bool
        switch opId {
        case 0...3: applies = { $0.kind == .video }
        case 10...12: applies = { $0.kind == .image }
        case 20: applies = { $0.kind == .csv } // CSV → XLSX
        case 21: applies = { $0.kind == .xlsx } // XLSX → CSV
        default: applies = { _ in false }
        }
        let queue = files.filter(applies)
        let skipped = files.count - queue.count
        guard !queue.isEmpty else { return }

        cancelRequested = false
        progress = 0
        etaSeconds = 0
        let batchStart = Date()

        batchTask = Task { [weak self] in
            var outputs: [String] = []
            var failed = 0
            for (i, file) in queue.enumerated() {
                guard let self else { return }
                if self.cancelRequested { break }
                self.stage = .running(RunInfo(label: label, index: i + 1, total: queue.count))

                let id = Engine.startJob(path: file.path, opId: opId, params: params)
                self.currentJobId = id

                var terminal = false
                while !terminal {
                    try? await Task.sleep(nanoseconds: 100_000_000)
                    if self.cancelRequested { Engine.cancel(id) }
                    let snap = Engine.poll(id)
                    let overall = (Double(i) + snap.pct) / Double(queue.count)
                    self.progress = overall
                    let elapsed = Date().timeIntervalSince(batchStart)
                    self.etaSeconds = overall > 0.02 ? elapsed * (1 - overall) / overall : 0
                    switch snap.status {
                    case .running: break
                    case .done: outputs.append(snap.outputPath); terminal = true
                    case .error: failed += 1; terminal = true
                    case .cancelled: terminal = true
                    }
                    if terminal { Engine.release(id); self.currentJobId = nil }
                }
                if self.cancelRequested { break }
            }
            guard let self else { return }
            self.finishBatch(outputs: outputs, failed: failed, skipped: skipped)
        }

        if let ms = autoCancelMs {
            Task { [weak self] in
                try? await Task.sleep(nanoseconds: ms * 1_000_000)
                self?.cancelJob()
            }
        }
    }

    private func finishBatch(outputs: [String], failed: Int, skipped: Int) {
        currentJobId = nil
        batchTask = nil
        if cancelRequested {
            // Return to the presets for this drop.
            if let info = presetInfo { stage = .presets(info) } else { stage = .drop }
            finishAutorun(ok: false, detail: "cancelled")
            return
        }
        stage = .done(DoneInfo(outputs: outputs, failed: failed, skipped: skipped))
        finishAutorun(
            ok: failed == 0,
            detail: "\(outputs.count) ok, \(failed) failed, \(skipped) skipped"
        )
    }

    func cancelJob() {
        cancelRequested = true
        if let id = currentJobId { Engine.cancel(id) }
    }

    func reset() {
        cancelRequested = true
        batchTask?.cancel()
        batchTask = nil
        if let id = currentJobId { Engine.cancel(id); Engine.release(id); currentJobId = nil }
        files = []
        singleProbe = nil
        presetInfo = nil
        stage = .drop
    }

    func reveal(_ paths: [String]) {
        let urls = paths.map { URL(fileURLWithPath: $0) }
        if urls.isEmpty { return }
        NSWorkspace.shared.activateFileViewerSelecting(urls)
    }

    private func finishAutorun(ok: Bool, detail: String) {
        guard autorunOpId != nil else { return }
        if let rf = resultFile {
            let line = (ok ? "ok\t" : "err\t") + detail + "\n"
            try? line.write(toFile: rf, atomically: true, encoding: .utf8)
        }
        NSApp.terminate(nil)
    }
}
