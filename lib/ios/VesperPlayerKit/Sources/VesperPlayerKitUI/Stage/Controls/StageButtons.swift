import SwiftUI
import VesperPlayerKit

@MainActor
public struct VesperStagePrimaryPlayButton: View {
    private let isPlaying: Bool
    private let label: String?
    private let style: VesperStageButtonStyle?
    private let action: () -> Void
    public init(isPlaying: Bool, label: String? = nil, style: VesperStageButtonStyle? = nil,
                action: @escaping () -> Void) {
        self.isPlaying = isPlaying
        self.label = label
        self.style = style
        self.action = action
    }
    public var body: some View {
        VesperStageIconButton(
            label: label ?? (isPlaying ? VesperPlayerStageStrings.pause : VesperPlayerStageStrings.play),
            variant: .primary, style: style, action: action
        ) { VesperStageIcon(isPlaying ? .pause : .play) }
    }
}

/// Decorative icon content inside an accessible action with a 44 point hit area.
@MainActor
public struct VesperStageIconButton<Icon: View>: View {
    @Environment(\.vesperPlayerStageSkin) private var skin
    private let label: String
    private let variant: VesperStageButtonVariant
    private let style: VesperStageButtonStyle?
    private let action: () -> Void
    private let icon: Icon
    public init(label: String, variant: VesperStageButtonVariant = .standard,
                style: VesperStageButtonStyle? = nil, action: @escaping () -> Void,
                @ViewBuilder icon: () -> Icon) {
        self.label = label
        self.variant = variant
        self.style = style
        self.action = action
        self.icon = icon()
    }
    public var body: some View {
        let appearance = style ?? skin.metrics.button(variant)
        Button(action: action) {
            icon
                .environment(\.vesperStageIconStyle, VesperStageIconStyle(size: appearance.iconSize, color: skin.colors.foreground))
                .font(.system(size: appearance.iconSize, weight: .semibold))
                .foregroundStyle(skin.colors.foreground)
                .frame(width: appearance.iconSize, height: appearance.iconSize)
                .allowsHitTesting(false)
                .accessibilityHidden(true)
                .frame(width: appearance.size, height: appearance.size)
                .background(skin.colors.buttonBackground.opacity(appearance.backgroundOpacity),
                    in: RoundedRectangle(cornerRadius: appearance.borderRadius))
                .frame(width: max(44, appearance.size), height: max(44, appearance.size))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityRepresentation {
            Button(label, action: action)
        }
    }
}

struct StageIconButton: View {
    let icon: VesperStageIconRole
    let label: String
    let variant: VesperStageButtonVariant
    let action: () -> Void
    var body: some View {
        VesperStageIconButton(label: label, variant: variant, action: action) { VesperStageIcon(icon) }
    }
}

struct StagePillButton: View {
    @Environment(\.vesperPlayerStageSkin) private var skin
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
            .foregroundStyle(skin.colors.foreground)
            .padding(.horizontal, compact ? 10 : 12)
            .padding(.vertical, compact ? 7 : 9)
            .background(skin.colors.buttonBackground.opacity(0.10), in: RoundedRectangle(cornerRadius: skin.metrics.standard.borderRadius))
        }
        .buttonStyle(.plain)
    }
}

struct StageChip: View {
    @Environment(\.vesperPlayerStageSkin) private var skin
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
                .foregroundStyle(skin.colors.foreground)
        }
        .padding(.horizontal, compact ? 8 : 10)
        .padding(.vertical, compact ? 5 : 7)
        .background(skin.colors.scrim.opacity(0.36), in: Capsule())
        .overlay(
            Capsule()
                .stroke(skin.colors.foreground.opacity(0.08), lineWidth: 1)
        )
    }
}
