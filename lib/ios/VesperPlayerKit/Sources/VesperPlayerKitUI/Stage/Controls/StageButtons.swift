import SwiftUI
import VesperPlayerKit
struct StagePrimaryPlayButton: View {
    let isPlaying: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: isPlaying ? "pause.fill" : "play.fill")
                .font(.system(size: 28, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 72, height: 72)
                .background(Color.white.opacity(0.14), in: Circle())
        }
        .buttonStyle(.plain)
    }
}

struct StageIconButton: View {
    let systemName: String
    var size: CGFloat = 52
    var iconSize: CGFloat = 18
    var backgroundOpacity: Double = 0.10
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: systemName)
                .font(.system(size: iconSize, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: size, height: size)
                .background(Color.white.opacity(backgroundOpacity), in: Circle())
        }
        .buttonStyle(.plain)
    }
}

struct StagePillButton: View {
    let systemName: String?
    let label: String
    var compact: Bool = false
    let action: () -> Void

    init(systemName: String? = nil, label: String, compact: Bool = false, action: @escaping () -> Void) {
        self.systemName = systemName
        self.label = label
        self.compact = compact
        self.action = action
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let systemName {
                    Image(systemName: systemName)
                        .font(.system(size: 13, weight: .semibold))
                }
                Text(label)
                    .font((compact ? Font.caption2 : .caption).weight(.semibold))
                    .lineLimit(1)
            }
            .foregroundStyle(.white)
            .padding(.horizontal, compact ? 10 : 12)
            .padding(.vertical, compact ? 7 : 9)
            .background(Color.white.opacity(0.10), in: Capsule())
        }
        .buttonStyle(.plain)
    }
}

struct StageChip: View {
    let label: String
    let accent: Color
    var compact: Bool = false

    var body: some View {
        HStack(spacing: compact ? 6 : 8) {
            Circle()
                .fill(accent)
                .frame(width: compact ? 6 : 8, height: compact ? 6 : 8)

            Text(label)
                .font((compact ? Font.caption2 : .caption).weight(.semibold))
                .foregroundStyle(.white)
        }
        .padding(.horizontal, compact ? 8 : 10)
        .padding(.vertical, compact ? 5 : 7)
        .background(Color.black.opacity(0.36), in: Capsule())
        .overlay(
            Capsule()
                .stroke(Color.white.opacity(0.08), lineWidth: 1)
        )
    }
}
