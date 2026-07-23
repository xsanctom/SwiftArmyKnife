// The per-state views.

import SwiftUI
import UniformTypeIdentifiers

// MARK: - Drop

struct DropZoneView: View {
    /// True while a file is hovered over the window — switches this same box
    /// from the resting look to the "ready to accept" look in place (identical
    /// geometry, so nothing shifts).
    var active: Bool = false

    var body: some View {
        RoundedRectangle(cornerRadius: Theme.cornerLarge, style: .continuous)
            .fill(active ? Theme.brand.opacity(0.10) : Color.primary.opacity(0.02))
            .overlay(
                RoundedRectangle(cornerRadius: Theme.cornerLarge, style: .continuous)
                    .strokeBorder(
                        active ? Theme.brand : Color.secondary.opacity(0.4),
                        style: StrokeStyle(lineWidth: 2, dash: active ? [] : [7, 6])
                    )
            )
            .overlay {
                VStack(spacing: 12) {
                    Image(systemName: "arrow.down.circle.fill")
                        .font(.system(size: 46, weight: .regular))
                        .foregroundStyle(active ? Theme.brand : Color.secondary)
                    Text(active ? "Drop to load" : "Drop a video or image")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(active ? Theme.brand : .primary)
                    // Kept present (just invisible when active) so the icon and
                    // title stay pinned in exactly the same spot.
                    Text("Convert · Compress · Resize · Audio · GIF")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .opacity(active ? 0 : 1)
                }
            }
            .padding(Theme.pad)
            .frame(maxHeight: .infinity)
    }
}

// MARK: - Probing

struct ProbingView: View {
    var body: some View {
        VStack(spacing: 14) {
            ProgressView().controlSize(.large)
            Text("Reading file…").foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(Theme.pad)
    }
}

// MARK: - Missing tools

struct MissingToolsView: View {
    @EnvironmentObject var model: AppModel
    @State private var copied = false

    var body: some View {
        StatusScaffold(
            systemImage: "shippingbox",
            tint: Theme.brand,
            title: "ffmpeg isn’t installed",
            subtitle: "Swift Army Knife uses ffmpeg to do its work. Install it, then re-check."
        ) {
            VStack(spacing: 12) {
                HStack(spacing: 8) {
                    Text(Engine.installCommand)
                        .font(.system(.callout, design: .monospaced))
                        .textSelection(.enabled)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(
                            RoundedRectangle(cornerRadius: Theme.cornerSmall)
                                .fill(Color.primary.opacity(0.06))
                        )
                    Button {
                        let pb = NSPasteboard.general
                        pb.clearContents()
                        pb.setString(Engine.installCommand, forType: .string)
                        copied = true
                    } label: {
                        Image(systemName: copied ? "checkmark" : "doc.on.doc")
                    }
                    .buttonStyle(.borderless)
                    .help("Copy")
                }
                Button("Re-check") { model.recheckTools() }
                    .buttonStyle(.borderedProminent)
            }
        }
    }
}

// MARK: - Unsupported

struct UnsupportedView: View {
    @EnvironmentObject var model: AppModel
    var body: some View {
        StatusScaffold(
            systemImage: "questionmark.folder",
            tint: .gray,
            title: "Unsupported file",
            subtitle: "Drop a video (mp4, mov, webm…) or an image (png, jpg, webp…)."
        ) {
            Button("Back") { model.reset() }.buttonStyle(.bordered)
        }
    }
}

// MARK: - Presets

struct PresetsView: View {
    @EnvironmentObject var model: AppModel
    let probe: Probe
    let presets: [Preset]

    @State private var showingAdvanced = false
    private let columns = [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)]

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            FileHeader(name: fileName, detail: summary, symbol: probe.isImage ? "photo.fill" : "video.fill") { model.reset() }

            LazyVGrid(columns: columns, spacing: 12) {
                ForEach(presets) { preset in
                    PresetCard(preset: preset) {
                        model.run(opId: preset.id, label: preset.label)
                    }
                }
            }

            Button {
                showingAdvanced = true
            } label: {
                Label("Advanced…", systemImage: "slider.horizontal.3")
                    .font(.callout)
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity)
        }
        .frame(maxHeight: .infinity, alignment: .top)
        .padding(Theme.pad)
        .sheet(isPresented: $showingAdvanced) {
            AdvancedSheet(presets: presets) { preset, params in
                model.run(opId: preset.id, label: preset.label, params: params)
            }
        }
    }

    private var fileName: String {
        URL(fileURLWithPath: model.currentFile ?? "").lastPathComponent
    }

    private var summary: String {
        var parts: [String] = []
        if probe.width > 0 { parts.append("\(probe.width)×\(probe.height)") }
        if probe.duration > 0 { parts.append(formatDuration(probe.duration)) }
        if !probe.videoCodec.isEmpty { parts.append(probe.videoCodec.uppercased()) }
        return parts.joined(separator: "  ·  ")
    }
}

/// Shared header: filename + metadata on the left, a close button on the right.
struct FileHeader: View {
    let name: String
    let detail: String
    var symbol: String = "video.fill"
    var onClose: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: symbol)
                .foregroundStyle(Theme.brand)
                .font(.body)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.headline)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button(action: onClose) {
                Image(systemName: "xmark.circle.fill")
                    .font(.title3)
                    .foregroundStyle(.tertiary)
            }
            .buttonStyle(.plain)
            .help("Close")
        }
    }
}

/// One preset button: tinted icon chip + label, with hover + press feedback.
struct PresetCard: View {
    let preset: Preset
    let action: () -> Void
    @State private var hovering = false

    private var tint: Color { Theme.opTint(preset.id) }

    var body: some View {
        Button(action: action) {
            VStack(spacing: 10) {
                Image(systemName: Theme.opIcon(preset.id))
                    .font(.system(size: 20, weight: .semibold))
                    .foregroundStyle(tint)
                    .frame(width: 44, height: 44)
                    .background(
                        RoundedRectangle(cornerRadius: 11, style: .continuous)
                            .fill(tint.opacity(0.15))
                    )
                Text(preset.label)
                    .font(.callout.weight(.medium))
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.center)
            }
            .frame(maxWidth: .infinity)
            .frame(height: 96)
            .background(
                RoundedRectangle(cornerRadius: Theme.cornerMedium, style: .continuous)
                    .fill(hovering ? tint.opacity(0.12) : Color.primary.opacity(0.05))
            )
            .overlay(
                RoundedRectangle(cornerRadius: Theme.cornerMedium, style: .continuous)
                    .strokeBorder(
                        hovering ? tint.opacity(0.55) : Color.primary.opacity(0.08),
                        lineWidth: 1
                    )
            )
            .contentShape(RoundedRectangle(cornerRadius: Theme.cornerMedium, style: .continuous))
        }
        .buttonStyle(ScaleButtonStyle())
        .onHover { hovering = $0 }
        .animation(.easeOut(duration: 0.12), value: hovering)
    }
}

// MARK: - Running

struct RunningView: View {
    @EnvironmentObject var model: AppModel
    let label: String

    var body: some View {
        VStack(spacing: 20) {
            VStack(spacing: 6) {
                Text(label)
                    .font(.title3.weight(.semibold))
                Text(fileName)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }

            VStack(spacing: 8) {
                ProgressView(value: model.progress)
                    .progressViewStyle(.linear)
                    .tint(Theme.brand)
                HStack {
                    Text("\(Int((model.progress * 100).rounded()))%")
                        .font(.callout.monospacedDigit().weight(.medium))
                    Spacer()
                    if model.etaSeconds > 0 {
                        Text("about \(formatDuration(model.etaSeconds)) left")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .frame(maxWidth: 300)

            Button("Cancel", role: .cancel) { model.cancelJob() }
                .buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(Theme.pad)
    }

    private var fileName: String {
        URL(fileURLWithPath: model.currentFile ?? "").lastPathComponent
    }
}

// MARK: - Done

struct DoneView: View {
    @EnvironmentObject var model: AppModel
    let path: String
    var body: some View {
        StatusScaffold(
            systemImage: "checkmark.circle.fill",
            tint: .green,
            title: "Done",
            subtitle: URL(fileURLWithPath: path).lastPathComponent
        ) {
            HStack(spacing: 10) {
                Button("Reveal in Finder") { model.reveal(path) }
                    .buttonStyle(.borderedProminent)
                Button("Do another") { model.reset() }
                    .buttonStyle(.bordered)
            }
        }
    }
}

// MARK: - Error

struct ErrorView: View {
    @EnvironmentObject var model: AppModel
    let message: String
    var body: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle().fill(Color.orange.opacity(0.15)).frame(width: 68, height: 68)
                Image(systemName: "exclamationmark.triangle.fill")
                    .font(.system(size: 28, weight: .medium))
                    .foregroundStyle(.orange)
            }
            Text("Something went wrong")
                .font(.title3.weight(.semibold))
            ScrollView {
                Text(message)
                    .font(.system(.caption, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
            }
            .frame(maxHeight: 120)
            .padding(10)
            .background(RoundedRectangle(cornerRadius: Theme.cornerSmall).fill(Color.primary.opacity(0.05)))
            Button("Back") { model.reset() }.buttonStyle(.bordered)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(Theme.pad)
    }
}

// MARK: - helpers

func formatDuration(_ seconds: Double) -> String {
    let total = Int(seconds.rounded())
    let m = total / 60
    let s = total % 60
    return String(format: "%d:%02d", m, s)
}
