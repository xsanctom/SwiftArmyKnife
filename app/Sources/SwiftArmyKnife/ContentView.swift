import SwiftUI

struct ContentView: View {
    @EnvironmentObject var model: AppModel
    @State private var dropTargeted = false

    var body: some View {
        content
            .frame(width: 400)
            .frame(minHeight: 400)
            .tint(Theme.brand)
            .animation(.easeInOut(duration: 0.22), value: stageKey)
            // The whole window accepts a new file in any state except while a
            // job is actively running (or mid-probe). A hovered file shows the
            // red "ready to accept" overlay below.
            // Non-idle states get the full-window opaque cover on hover. The
            // idle state instead toggles its own box (see `content`), so the
            // resting → active change is a perfect in-place replacement.
            .overlay {
                if dropTargeted && droppable && !isDropState {
                    DropOverlay()
                }
            }
            .dropDestination(for: URL.self) { urls, _ in
                guard droppable, !urls.isEmpty else { return false }
                model.handleDrop(urls.map { $0.path })
                return true
            } isTargeted: { dropTargeted = $0 }
            .onAppear { model.start() }
    }

    @ViewBuilder private var content: some View {
        Group {
            switch model.stage {
            case .drop:
                DropZoneView(active: dropTargeted)
            case .probing:
                ProbingView()
            case .missingTools:
                MissingToolsView()
            case .unsupported:
                UnsupportedView()
            case let .presets(info):
                PresetsView(info: info)
            case let .running(run):
                RunningView(run: run)
            case let .done(done):
                DoneView(done: done)
            case let .error(message):
                ErrorView(message: message)
            }
        }
        .id(stageKey)
        .transition(.opacity)
    }

    /// Drops are accepted unless a job is running or a probe is in flight.
    private var droppable: Bool {
        switch model.stage {
        case .running, .probing: return false
        default: return true
        }
    }

    private var isDropState: Bool {
        if case .drop = model.stage { return true }
        return false
    }

    // A stable key so the cross-fade + height animation fire on state changes.
    private var stageKey: String {
        switch model.stage {
        case .drop: return "drop"
        case .probing: return "probing"
        case .missingTools: return "missingTools"
        case .unsupported: return "unsupported"
        case .presets: return "presets"
        case .running: return "running"
        case .done: return "done"
        case .error: return "error"
        }
    }
}

/// The red "ready to accept" state shown over the whole window while a file is
/// dragged over it.
struct DropOverlay: View {
    var body: some View {
        ZStack {
            // Fully opaque so it completely covers the state beneath — no
            // blended/overlapping text. Window-matched base + a subtle red wash.
            Color(nsColor: .windowBackgroundColor)
            Theme.brand.opacity(0.10)
            VStack(spacing: 12) {
                Image(systemName: "arrow.down.circle.fill")
                    .font(.system(size: 48, weight: .regular))
                    .foregroundStyle(Theme.brand)
                Text("Drop to load")
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(Theme.brand)
            }
        }
        .overlay(
            RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(Theme.brand, lineWidth: 3)
                .padding(5)
        )
    }
}
