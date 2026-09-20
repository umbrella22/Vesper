import SwiftUI

/// Action or feedback represented by an icon, independent of the image asset.
public enum VesperStageIconRole: CaseIterable, Hashable, Sendable {
    case play
    case pause
    case fullscreen
    case exitFullscreen
    case navigateBack
    case more
    case brightness
    case volume
    case speed
}

public enum VesperStageButtonVariant: CaseIterable, Sendable {
    case standard, toolbar, navigation, compact, compactFullscreen, expanded, expandedFullscreen, primary
}

public struct VesperStageIconStyle {
    public let size: CGFloat
    public let color: Color
    public init(size: CGFloat, color: Color) {
        self.size = size
        self.color = color
    }
}

/// SF Symbols defaults. Use the skin's iconBuilder for asset images or custom views.
public struct VesperPlayerStageIcons {
    public var play: String
    public var pause: String
    public var fullscreen: String
    public var exitFullscreen: String
    public var navigateBack: String
    public var more: String
    public var brightness: String
    public var volume: String
    public var speed: String
    public init(
        play: String = "play.fill",
        pause: String = "pause.fill",
        fullscreen: String = "arrow.up.left.and.arrow.down.right",
        exitFullscreen: String = "arrow.down.right.and.arrow.up.left",
        navigateBack: String = "chevron.left",
        more: String = "ellipsis",
        brightness: String = "sun.max.fill",
        volume: String = "speaker.wave.2.fill",
        speed: String = "speedometer"
    ) {
        self.play = play
        self.pause = pause
        self.fullscreen = fullscreen
        self.exitFullscreen = exitFullscreen
        self.navigateBack = navigateBack
        self.more = more
        self.brightness = brightness
        self.volume = volume
        self.speed = speed
    }
    public func resolve(_ role: VesperStageIconRole) -> String {
        switch role {
        case .play: return play
        case .pause: return pause
        case .fullscreen: return fullscreen
        case .exitFullscreen: return exitFullscreen
        case .navigateBack: return navigateBack
        case .more: return more
        case .brightness: return brightness
        case .volume: return volume
        case .speed: return speed
        }
    }
}

public struct VesperStageColors {
    public var foreground: Color
    public var secondaryForeground: Color
    public var background: Color
    public var scrim: Color
    public var buttonBackground: Color
    public var accent: Color
    public var timelineStart: Color
    public var timelineEnd: Color
    public var timelineInactive: Color
    public var timelineThumb: Color
    public var hudBackground: Color
    public var hudForeground: Color
    public init(
        foreground: Color = .white,
        secondaryForeground: Color = .white.opacity(0.70),
        background: Color = .black,
        scrim: Color = .black,
        buttonBackground: Color = .white,
        accent: Color = Color(red: 1.0, green: 0.71, blue: 0.33),
        timelineStart: Color = Color(red: 1.0, green: 0.42, blue: 0.56),
        timelineEnd: Color = Color(red: 1.0, green: 0.71, blue: 0.33),
        timelineInactive: Color = .white,
        timelineThumb: Color = .white,
        hudBackground: Color = .black.opacity(0.72),
        hudForeground: Color = .white
    ) {
        self.foreground = foreground
        self.secondaryForeground = secondaryForeground
        self.background = background
        self.scrim = scrim
        self.buttonBackground = buttonBackground
        self.accent = accent
        self.timelineStart = timelineStart
        self.timelineEnd = timelineEnd
        self.timelineInactive = timelineInactive
        self.timelineThumb = timelineThumb
        self.hudBackground = hudBackground
        self.hudForeground = hudForeground
    }
}

/// Visual size. Buttons preserve a minimum 44 point touch target.
public struct VesperStageButtonStyle {
    public let size: CGFloat
    public let iconSize: CGFloat
    public let backgroundOpacity: Double
    public let borderRadius: CGFloat
    public init(size: CGFloat = 52, iconSize: CGFloat = 18,
                backgroundOpacity: Double = 0.10, borderRadius: CGFloat = 999) {
        precondition(size.isFinite && size > 0)
        precondition(iconSize.isFinite && iconSize > 0)
        precondition((0...1).contains(backgroundOpacity))
        precondition(borderRadius.isFinite && borderRadius >= 0)
        self.size = size
        self.iconSize = iconSize
        self.backgroundOpacity = backgroundOpacity
        self.borderRadius = borderRadius
    }
}

public struct VesperStageMetrics {
    public var standard: VesperStageButtonStyle
    public var toolbar: VesperStageButtonStyle
    public var navigation: VesperStageButtonStyle
    public var compact: VesperStageButtonStyle
    public var compactFullscreen: VesperStageButtonStyle
    public var expanded: VesperStageButtonStyle
    public var expandedFullscreen: VesperStageButtonStyle
    public var primary: VesperStageButtonStyle
    public let buttonSpacing: CGFloat
    public let hudIconSize: CGFloat
    public let hudBorderRadius: CGFloat
    public let timelineTrackHeight: CGFloat
    public let timelineThumbSize: CGFloat
    public let timelineLargeThumbSize: CGFloat
    public init(
        standard: VesperStageButtonStyle = .init(),
        toolbar: VesperStageButtonStyle = .init(size: 38, iconSize: 22, backgroundOpacity: 0),
        navigation: VesperStageButtonStyle = .init(size: 38, iconSize: 19, backgroundOpacity: 0),
        compact: VesperStageButtonStyle = .init(size: 38, iconSize: 17, backgroundOpacity: 0),
        compactFullscreen: VesperStageButtonStyle = .init(size: 38, iconSize: 18, backgroundOpacity: 0),
        expanded: VesperStageButtonStyle = .init(size: 38, iconSize: 17, backgroundOpacity: 0),
        expandedFullscreen: VesperStageButtonStyle = .init(size: 34, iconSize: 17, backgroundOpacity: 0),
        primary: VesperStageButtonStyle = .init(size: 72, iconSize: 28, backgroundOpacity: 0.14),
        buttonSpacing: CGFloat = 8,
        hudIconSize: CGFloat = 23,
        hudBorderRadius: CGFloat = 999,
        timelineTrackHeight: CGFloat = 4,
        timelineThumbSize: CGFloat = 12,
        timelineLargeThumbSize: CGFloat = 14
    ) {
        precondition(buttonSpacing.isFinite && buttonSpacing >= 0)
        precondition(hudIconSize.isFinite && hudIconSize > 0)
        precondition(hudBorderRadius.isFinite && hudBorderRadius >= 0)
        precondition(timelineTrackHeight.isFinite && timelineTrackHeight > 0)
        precondition(timelineThumbSize.isFinite && timelineThumbSize > 0)
        precondition(timelineLargeThumbSize.isFinite && timelineLargeThumbSize > 0)
        self.standard = standard
        self.toolbar = toolbar
        self.navigation = navigation
        self.compact = compact
        self.compactFullscreen = compactFullscreen
        self.expanded = expanded
        self.expandedFullscreen = expandedFullscreen
        self.primary = primary
        self.buttonSpacing = buttonSpacing
        self.hudIconSize = hudIconSize
        self.hudBorderRadius = hudBorderRadius
        self.timelineTrackHeight = timelineTrackHeight
        self.timelineThumbSize = timelineThumbSize
        self.timelineLargeThumbSize = timelineLargeThumbSize
    }
    public func button(_ variant: VesperStageButtonVariant) -> VesperStageButtonStyle {
        switch variant {
        case .standard: return standard
        case .toolbar: return toolbar
        case .navigation: return navigation
        case .compact: return compact
        case .compactFullscreen: return compactFullscreen
        case .expanded: return expanded
        case .expandedFullscreen: return expandedFullscreen
        case .primary: return primary
        }
    }
}

/// A nil builder result falls back to the configured SF Symbol for that role.
public typealias VesperStageIconBuilder = @MainActor (VesperStageIconRole, VesperStageIconStyle) -> AnyView?

/// Presentation-only configuration; it never crosses the player bridge or FFI.
public struct VesperPlayerStageSkin {
    public var icons: VesperPlayerStageIcons
    public var colors: VesperStageColors
    public var metrics: VesperStageMetrics
    public var iconBuilder: VesperStageIconBuilder?
    public init(icons: VesperPlayerStageIcons = .init(), colors: VesperStageColors = .init(),
                metrics: VesperStageMetrics = .init(), iconBuilder: VesperStageIconBuilder? = nil) {
        self.icons = icons
        self.colors = colors
        self.metrics = metrics
        self.iconBuilder = iconBuilder
    }
}

private struct VesperPlayerStageSkinKey: EnvironmentKey {
    static let defaultValue = VesperPlayerStageSkin()
}
private struct VesperStageIconStyleKey: EnvironmentKey {
    static let defaultValue: VesperStageIconStyle? = nil
}

public extension EnvironmentValues {
    /// Active skin for standalone controls and host content inside a Stage.
    var vesperPlayerStageSkin: VesperPlayerStageSkin {
        get { self[VesperPlayerStageSkinKey.self] }
        set { self[VesperPlayerStageSkinKey.self] = newValue }
    }
}
extension EnvironmentValues {
    var vesperStageIconStyle: VesperStageIconStyle? {
        get { self[VesperStageIconStyleKey.self] }
        set { self[VesperStageIconStyleKey.self] = newValue }
    }
}

public extension View {
    /// Scopes standalone controls. Each Stage establishes its own skin scope.
    @MainActor func vesperPlayerStageSkin(_ skin: VesperPlayerStageSkin) -> some View {
        environment(\.vesperPlayerStageSkin, skin)
    }
}

/// A decorative action icon, resolved from the active skin.
@MainActor
public struct VesperStageIcon: View {
    @Environment(\.vesperPlayerStageSkin) private var skin
    @Environment(\.vesperStageIconStyle) private var inheritedStyle
    private let role: VesperStageIconRole
    private let style: VesperStageIconStyle?
    public init(_ role: VesperStageIconRole, style: VesperStageIconStyle? = nil) {
        self.role = role
        self.style = style
    }
    public var body: some View {
        let effective = style ?? inheritedStyle
            ?? VesperStageIconStyle(size: skin.metrics.standard.iconSize, color: skin.colors.foreground)
        Group {
            if let custom = skin.iconBuilder?(role, effective) {
                custom
            } else {
                Image(systemName: skin.icons.resolve(role))
            }
        }
        .font(.system(size: effective.size, weight: .semibold))
        .foregroundStyle(effective.color)
        .frame(width: effective.size, height: effective.size)
        .allowsHitTesting(false)
        .accessibilityRepresentation { EmptyView() }
        .accessibilityHidden(true)
    }
}
