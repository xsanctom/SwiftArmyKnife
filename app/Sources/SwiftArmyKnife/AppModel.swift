// App state machine + orchestration of engine calls off the main thread.

import AppKit
import SwiftUI

@MainActor
final class AppModel: ObservableObject {
    enum Stage {
        case drop
        case probing
        case missingTools
        case unsupported
        case presets(Probe, [Preset])
        case running(String) // op label
        case done(String) // output path
        case error(String)
    }

    @Published var stage: Stage = .drop
    @Published var progress: Double = 0 // 0...1
    @Published var etaSeconds: Double = 0
    private(set) var currentFile: String?

    private var lastProbe: Probe?
    private var jobId: UInt64?
    private var pollTask: Task<Void, Never>?

    // --- headless verification hooks (no effect in normal interactive use) ---
    // Driven by CLI args so the app can be launched via `open --args`:
    //   --preload <file> --autorun <opId> --result <path>
    private var autorunOpId: UInt32?
    private var resultFile: String?
    private var autoCancelMs: UInt64?
    private var autorunParams = AdvancedParams()

    /// Called from ContentView.onAppear. Auto-loads a preloaded file if the
    /// verification flags/env are present; otherwise a no-op.
    func start() {
        let env = ProcessInfo.processInfo.environment
        var preload = env["SAK_PRELOAD"]
        autorunOpId = env["SAK_AUTORUN"].flatMap { UInt32($0) }
        resultFile = env["SAK_RESULT_FILE"]

        let args = CommandLine.arguments
        var i = 0
        while i < args.count {
            let next = i + 1 < args.count ? args[i + 1] : nil
            switch args[i] {
            case "--preload": if let n = next { preload = n; i += 1 }
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

        // No ffmpeg/ffprobe → nothing works; say so clearly instead of
        // mislabelling every dropped file as "not a video".
        if !Engine.toolsReady {
            stage = .missingTools
            return
        }

        if let f = preload, FileManager.default.fileExists(atPath: f) {
            handleDrop(f)
        }
    }

    /// Re-check for the tools (after the user installs them) and leave the
    /// missing-tools screen if they're now present.
    func recheckTools() {
        if Engine.toolsReady { stage = .drop }
    }

    func handleDrop(_ path: String) {
        // A file can be dropped from any state; if the tools vanished, say so
        // rather than probe-failing into a misleading "not a video".
        guard Engine.toolsReady else {
            stage = .missingTools
            return
        }
        currentFile = path
        stage = .probing
        // Main-actor Task; the blocking FFI call hops to a detached task.
        Task {
            let probe = await Task.detached { Engine.probe(path) }.value
            if probe.isMedia {
                lastProbe = probe
                stage = .presets(probe, Engine.presets(for: probe))
                if let op = autorunOpId { run(opId: op, label: "auto", params: autorunParams) }
            } else {
                stage = .unsupported
                finishAutorun(ok: false, detail: "unsupported file")
            }
        }
    }

    func run(opId: UInt32, label: String, params: AdvancedParams = AdvancedParams()) {
        guard let file = currentFile else { return }
        progress = 0
        etaSeconds = 0
        stage = .running(label)
        jobId = Engine.startJob(path: file, opId: opId, params: params)
        // Poll via Swift concurrency (not NSTimer — the run loop isn't reliably
        // servicing timers in every launch context).
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 100_000_000) // 0.1s
                guard let self else { return }
                if self.pollOnce() { return } // terminal state reached
            }
        }

        // Test hook: auto-cancel after a delay to exercise the cancel path.
        if let ms = autoCancelMs {
            Task { [weak self] in
                try? await Task.sleep(nanoseconds: ms * 1_000_000)
                self?.cancelJob()
            }
        }
    }

    func cancelJob() {
        if let id = jobId { Engine.cancel(id) }
        // The poll loop observes the cancelled status and returns to presets.
    }

    /// One poll tick. Returns true when the job reached a terminal state.
    @discardableResult
    private func pollOnce() -> Bool {
        guard let id = jobId else { return true }
        let snap = Engine.poll(id)
        progress = snap.pct
        etaSeconds = snap.etaSeconds
        switch snap.status {
        case .running:
            return false
        case .done:
            finishJob()
            stage = .done(snap.outputPath)
            finishAutorun(ok: true, detail: snap.outputPath)
            return true
        case .error:
            finishJob()
            stage = .error(snap.error)
            finishAutorun(ok: false, detail: snap.error)
            return true
        case .cancelled:
            finishJob()
            if let probe = lastProbe {
                stage = .presets(probe, Engine.presets(for: probe))
            } else {
                stage = .drop
            }
            // In interactive use autorunOpId is nil, so this is a no-op and we
            // simply return to presets. Under the test hook it records + quits.
            finishAutorun(ok: false, detail: "cancelled")
            return true
        }
    }

    private func finishJob() {
        pollTask?.cancel()
        pollTask = nil
        if let id = jobId {
            Engine.release(id)
            jobId = nil
        }
    }

    func reset() {
        finishJob()
        currentFile = nil
        lastProbe = nil
        stage = .drop
    }

    func reveal(_ path: String) {
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)])
    }

    /// When driven by SAK_AUTORUN, write the outcome and quit so a test can read it.
    private func finishAutorun(ok: Bool, detail: String) {
        guard autorunOpId != nil else { return }
        if let rf = resultFile {
            let line = (ok ? "ok\t" : "err\t") + detail + "\n"
            try? line.write(toFile: rf, atomically: true, encoding: .utf8)
        }
        NSApp.terminate(nil)
    }
}
