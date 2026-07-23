// Small design system: brand color, per-operation tints, spacing, and the
// reusable pieces (status layout, preset card style) the views share.

import SwiftUI

enum Theme {
    /// Swift-Army-knife red. Change here to re-brand the whole app.
    static let brand = Color(red: 0.82, green: 0.18, blue: 0.15)

    static let pad: CGFloat = 24
    static let cornerLarge: CGFloat = 18
    static let cornerMedium: CGFloat = 12
    static let cornerSmall: CGFloat = 8

    /// Distinct accent per operation so the 2×2 grid reads at a glance.
    static func opTint(_ opId: UInt32) -> Color {
        switch opId {
        case 0: return .blue // Convert
        case 1: return .purple // Compress
        case 2: return .pink // Extract audio
        case 3: return .orange // GIF
        default: return brand
        }
    }

    static func opIcon(_ opId: UInt32) -> String {
        switch opId {
        case 0: return "film"
        case 1: return "arrow.down.right.and.arrow.up.left"
        case 2: return "waveform"
        case 3: return "photo.stack"
        default: return "wand.and.stars"
        }
    }
}

// MARK: - Reusable status layout (done / error / unsupported / missing tools)

struct StatusScaffold<Actions: View>: View {
    let systemImage: String
    let tint: Color
    let title: String
    var subtitle: String?
    @ViewBuilder var actions: () -> Actions

    var body: some View {
        VStack(spacing: 14) {
            ZStack {
                Circle().fill(tint.opacity(0.15)).frame(width: 68, height: 68)
                Image(systemName: systemImage)
                    .font(.system(size: 30, weight: .medium))
                    .foregroundStyle(tint)
            }
            Text(title)
                .font(.title3.weight(.semibold))
                .multilineTextAlignment(.center)
            if let subtitle {
                Text(subtitle)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }
            actions().padding(.top, 2)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(Theme.pad)
    }
}

// MARK: - Press-scale button style (canonical, no nested view)

struct ScaleButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .animation(.easeOut(duration: 0.10), value: configuration.isPressed)
    }
}
