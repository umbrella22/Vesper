import SwiftUI
import VesperPlayerKit

struct ExamplePlayerHeader: View {
    let sourceLabel: String
    let subtitle: String
    let palette: ExampleHostPalette

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Vesper")
                .font(.system(size: 38, weight: .black, design: .rounded))
                .foregroundStyle(palette.title)

            Text(sourceLabel)
                .font(.headline.weight(.semibold))
                .foregroundStyle(palette.title.opacity(0.94))
                .lineLimit(1)

            Text(subtitle)
                .font(.subheadline)
                .foregroundStyle(palette.body)
                .lineLimit(2)
        }
    }
}

struct ExampleSourceSection: View {
    let palette: ExampleHostPalette
    let themeMode: ExampleThemeMode
    @Binding var remoteStreamUrl: String
    let hostMessage: String?
    let dashDemoEnabled: Bool
    let dashDemoNote: String?
    let onThemeModeChange: (ExampleThemeMode) -> Void
    let onPickVideo: () -> Void
    let onUseHlsDemo: () -> Void
    let onUseDashDemo: () -> Void
    let onUseLiveDvrAcceptance: () -> Void
    let onOpenRemote: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(ExampleI18n.sourcesTitle)
                .font(.title3.weight(.bold))
                .foregroundStyle(palette.title)

            Text(ExampleI18n.sourcesSubtitle)
                .font(.footnote)
                .foregroundStyle(palette.body)

            if let hostMessage {
                Text(hostMessage)
                    .font(.caption)
                    .foregroundStyle(Color.red.opacity(0.92))
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    sourceActionButton(ExampleI18n.pickVideo, action: onPickVideo)
                    sourceActionButton(ExampleI18n.useHlsDemo, action: onUseHlsDemo)
                    sourceActionButton(ExampleI18n.useLiveDvrAcceptance, action: onUseLiveDvrAcceptance)
                    sourceActionButton(
                        ExampleI18n.useDashDemo,
                        enabled: dashDemoEnabled,
                        action: onUseDashDemo
                    )
                }
            }

            if let dashDemoNote {
                Text(dashDemoNote)
                    .font(.footnote)
                    .foregroundStyle(palette.body)
                    .lineSpacing(4)
            }

            VStack(alignment: .leading, spacing: 10) {
                Text(ExampleI18n.themeTitle)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(palette.title)

                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 10) {
                        ForEach(ExampleThemeMode.allCases) { mode in
                            ExampleThemeModeChip(
                                mode: mode,
                                selected: themeMode == mode,
                                palette: palette,
                                onClick: { onThemeModeChange(mode) }
                            )
                        }
                    }
                }
            }

            TextField(ExampleI18n.remoteUrlPlaceholder, text: $remoteStreamUrl)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(palette.fieldBackground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                .foregroundStyle(palette.fieldText)

            Button(action: onOpenRemote) {
                Text(ExampleI18n.openRemoteUrl)
                    .font(.headline)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 14)
            }
            .buttonStyle(.plain)
            .background(palette.primaryAction, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
            .foregroundStyle(.white)
        }
        .padding(18)
        .background(palette.sectionBackground, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(palette.sectionStroke, lineWidth: 1)
        )
    }

    @ViewBuilder
    private func sourceActionButton(
        _ title: String,
        enabled: Bool = true,
        action: @escaping () -> Void
    ) -> some View {
        Button(title, action: action)
            .buttonStyle(.plain)
            .font(.subheadline.weight(.semibold))
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(.white.opacity(0.08), in: Capsule())
            .foregroundStyle(palette.title)
            .opacity(enabled ? 1.0 : 0.46)
            .disabled(!enabled)
    }
}

struct ExamplePlaylistSection: View {
    let palette: ExampleHostPalette
    let playlistQueue: [VesperPlaylistQueueItemState]
    let onFocusPlaylistItem: (String) -> Void

    var body: some View {
        ExampleSectionShell(
            palette: palette,
            title: ExampleI18n.playlistTitle,
            subtitle: ExampleI18n.playlistSubtitle
        ) {
            if playlistQueue.isEmpty {
                Text(ExampleI18n.playlistEmpty)
                    .font(.footnote)
                    .foregroundStyle(palette.body)
            } else {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(playlistQueue, id: \.item.itemId) { item in
                        PlaylistQueueRow(
                            label: item.item.source.label,
                            hint: item.isActive ? ExampleI18n.playlistStatusCurrent : playlistHintLabel(item.viewportHint),
                            active: item.isActive,
                            palette: palette,
                            onClick: { onFocusPlaylistItem(item.item.itemId) }
                        )
                    }
                }
            }
        }
    }
}

private struct PlaylistQueueRow: View {
    let label: String
    let hint: String
    let active: Bool
    let palette: ExampleHostPalette
    let onClick: () -> Void

    var body: some View {
        Button(action: onClick) {
            VStack(alignment: .leading, spacing: 4) {
                Text(label)
                    .font(.body.weight(.semibold))
                    .lineLimit(1)
                Text(hint)
                    .font(.caption.weight(.semibold))
                    .lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .foregroundStyle(active ? Color.white : palette.title)
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(
                active ? AnyShapeStyle(palette.primaryAction) : AnyShapeStyle(palette.fieldBackground),
                in: RoundedRectangle(cornerRadius: 18, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .stroke(active ? Color.clear : palette.sectionStroke, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}

struct ExampleResilienceSection: View {
    let palette: ExampleHostPalette
    let selectedProfile: ExampleResilienceProfile
    let isApplyingProfile: Bool
    let onApplyProfile: (ExampleResilienceProfile) -> Void

    var body: some View {
        let policy = selectedProfile.policy
        ExampleSectionShell(
            palette: palette,
            title: ExampleI18n.resilienceTitle,
            subtitle: ExampleI18n.resilienceSubtitle
        ) {
            VStack(alignment: .leading, spacing: 14) {
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: 112), spacing: 10)],
                    alignment: .leading,
                    spacing: 10
                ) {
                    ForEach(ExampleResilienceProfile.allCases) { profile in
                        ExampleResilienceChip(
                            profile: profile,
                            selected: profile == selectedProfile,
                            palette: palette,
                            onClick: { onApplyProfile(profile) }
                        )
                    }
                }

                Text(selectedProfile.subtitle)
                    .font(.body)
                    .foregroundStyle(palette.body)
                    .lineSpacing(5)

                if isApplyingProfile {
                    ExampleStatusPill(
                        label: ExampleI18n.resilienceApplying,
                        palette: palette
                    )
                }

                VStack(alignment: .leading, spacing: 4) {
                    ExampleFactRow(
                        label: ExampleI18n.resilienceFactBuffering,
                        value: resilienceBufferingValue(policy.buffering),
                        palette: palette
                    )
                    ExampleFactRow(
                        label: ExampleI18n.resilienceFactRetry,
                        value: resilienceRetryValue(policy.retry),
                        palette: palette
                    )
                    ExampleFactRow(
                        label: ExampleI18n.resilienceFactCache,
                        value: resilienceCacheValue(policy.cache),
                        palette: palette
                    )
                }
            }
        }
    }
}

struct ExampleThemeModeChip: View {
    let mode: ExampleThemeMode
    let selected: Bool
    let palette: ExampleHostPalette
    let onClick: () -> Void

    var body: some View {
        Button(action: onClick) {
            HStack(spacing: 6) {
                Image(systemName: mode.systemImage)
                    .font(.system(size: 13, weight: .semibold))
                Text(mode.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(1)
            }
            .foregroundStyle(selected ? Color.white : palette.title)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(
                selected
                    ? AnyShapeStyle(palette.primaryAction)
                    : AnyShapeStyle(palette.fieldBackground),
                in: Capsule()
            )
            .overlay(
                Capsule()
                    .stroke(selected ? Color.clear : palette.sectionStroke, lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }
}

struct ExampleSectionShell<Content: View>: View {
    let palette: ExampleHostPalette
    let title: String
    let subtitle: String
    let accent: Color
    let content: () -> Content

    init(
        palette: ExampleHostPalette,
        title: String,
        subtitle: String,
        accent: Color = Color(red: 0.09, green: 0.13, blue: 0.20),
        @ViewBuilder content: @escaping () -> Content
    ) {
        self.palette = palette
        self.title = title
        self.subtitle = subtitle
        self.accent = accent
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(title)
                .font(.title3.weight(.bold))
                .foregroundStyle(palette.title)

            Text(subtitle)
                .font(.footnote)
                .foregroundStyle(palette.body)
                .lineSpacing(4)

            RoundedRectangle(cornerRadius: 999, style: .continuous)
                .fill(accent)
                .frame(width: 42, height: 4)

            content()
        }
        .padding(18)
        .background(palette.sectionBackground, in: RoundedRectangle(cornerRadius: 24, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .stroke(palette.sectionStroke, lineWidth: 1)
        )
    }
}

struct ExampleFactRow: View {
    let label: String
    let value: String
    let palette: ExampleHostPalette

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(palette.body)
                .textCase(.uppercase)

            Text(value)
                .font(.subheadline)
                .foregroundStyle(palette.title)
                .lineLimit(2)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 4)
    }
}

struct ExampleStatusPill: View {
    let label: String
    let palette: ExampleHostPalette

    var body: some View {
        Text(label)
            .font(.caption.weight(.semibold))
            .foregroundStyle(palette.title)
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(palette.fieldBackground, in: Capsule())
            .overlay(
                Capsule()
                    .stroke(palette.sectionStroke, lineWidth: 1)
            )
    }
}

private struct ExampleResilienceChip: View {
    let profile: ExampleResilienceProfile
    let selected: Bool
    let palette: ExampleHostPalette
    let onClick: () -> Void

    var body: some View {
        Button(action: onClick) {
            Text(profile.title)
                .font(.subheadline.weight(.semibold))
                .lineLimit(1)
                .frame(maxWidth: .infinity)
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .foregroundStyle(selected ? Color.white : palette.title)
                .background(
                    selected
                        ? AnyShapeStyle(palette.primaryAction)
                        : AnyShapeStyle(palette.fieldBackground),
                    in: Capsule()
                )
                .overlay(
                    Capsule()
                        .stroke(selected ? Color.clear : palette.sectionStroke, lineWidth: 1)
                )
        }
        .buttonStyle(.plain)
        .disabled(selected)
    }
}
