// The "Advanced…" sheet: pick an operation and tune its parameters, then run.

import SwiftUI

struct AdvancedSheet: View {
    @Environment(\.dismiss) private var dismiss
    let presets: [Preset]
    var onRun: (Preset, AdvancedParams) -> Void

    @State private var selection: UInt32
    @State private var params = AdvancedParams()

    init(presets: [Preset], onRun: @escaping (Preset, AdvancedParams) -> Void) {
        self.presets = presets
        self.onRun = onRun
        _selection = State(initialValue: presets.first?.id ?? 0)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Advanced").font(.headline)

            Picker("Operation", selection: $selection) {
                ForEach(presets) { Text($0.label).tag($0.id) }
            }
            .pickerStyle(.menu)

            Divider()

            controls

            Spacer(minLength: 8)

            HStack {
                Button("Cancel") { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Spacer()
                Button("Run") {
                    if let preset = presets.first(where: { $0.id == selection }) {
                        onRun(preset, params)
                    }
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 360)
    }

    @ViewBuilder private var controls: some View {
        switch selection {
        case 0: convertControls
        case 1: compressControls
        case 2: audioControls
        case 3: gifControls
        default: EmptyView()
        }
    }

    // MARK: per-op controls

    private var convertControls: some View {
        VStack(alignment: .leading, spacing: 14) {
            labeledPicker("Codec", selection: $params.videoCodec) {
                Text("H.264").tag(UInt32(0))
                Text("HEVC").tag(UInt32(1))
            }
            sliderRow("Quality (CRF)", value: crfBinding, range: 18...28, display: "\(params.crf)")
            Picker("Max height", selection: $params.maxHeight) {
                Text("Original").tag(UInt32(0))
                Text("1080p").tag(UInt32(1080))
                Text("720p").tag(UInt32(720))
                Text("480p").tag(UInt32(480))
            }
            Toggle("Hardware acceleration", isOn: $params.hwAccel)
        }
    }

    private var compressControls: some View {
        VStack(alignment: .leading, spacing: 14) {
            labeledPicker("Mode", selection: $params.compressMode) {
                Text("Target size").tag(UInt32(0))
                Text("Quality").tag(UInt32(1))
            }
            if params.compressMode == 0 {
                sliderRow(
                    "Target size", value: $params.targetMB, range: 5...200, step: 5,
                    display: "\(Int(params.targetMB)) MB"
                )
            } else {
                sliderRow("Quality (CRF)", value: crfBinding, range: 18...32, display: "\(params.crf)")
            }
        }
    }

    private var audioControls: some View {
        VStack(alignment: .leading, spacing: 14) {
            labeledPicker("Format", selection: $params.audioFormat) {
                Text("MP3").tag(UInt32(0))
                Text("M4A (AAC)").tag(UInt32(1))
            }
            Picker("Bitrate", selection: $params.audioBitrateK) {
                Text("128 kbps").tag(UInt32(128))
                Text("192 kbps").tag(UInt32(192))
                Text("256 kbps").tag(UInt32(256))
                Text("320 kbps").tag(UInt32(320))
            }
        }
    }

    private var gifControls: some View {
        VStack(alignment: .leading, spacing: 14) {
            Stepper(
                "Frame rate: \(params.gifFps) fps",
                value: Binding(get: { Int(params.gifFps) }, set: { params.gifFps = UInt32($0) }),
                in: 5...30
            )
            Picker("Width", selection: $params.gifWidth) {
                Text("320 px").tag(UInt32(320))
                Text("480 px").tag(UInt32(480))
                Text("640 px").tag(UInt32(640))
            }
        }
    }

    // MARK: helpers

    private var crfBinding: Binding<Double> {
        Binding(get: { Double(params.crf) }, set: { params.crf = UInt32($0.rounded()) })
    }

    private func labeledPicker<Content: View>(
        _ label: String, selection: Binding<UInt32>, @ViewBuilder content: () -> Content
    ) -> some View {
        Picker(label, selection: selection, content: content)
            .pickerStyle(.segmented)
    }

    private func sliderRow(
        _ label: String, value: Binding<Double>, range: ClosedRange<Double>, step: Double = 1,
        display: String
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(label)
                Spacer()
                Text(display).foregroundStyle(.secondary).font(.callout.monospacedDigit())
            }
            Slider(value: value, in: range, step: step)
        }
    }
}
