import SwiftUI

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
    let onThemeModeChange: (ExampleThemeMode) -> Void
    let onPickFromPhotos: () -> Void
    let onUseHlsDemo: () -> Void
    let onOpenRemote: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Sources")
                .font(.title3.weight(.bold))
                .foregroundStyle(palette.title)

            Text("Use the example host to switch among local files, HLS demo streams, and custom remote URLs.")
                .font(.footnote)
                .foregroundStyle(palette.body)

            if let hostMessage {
                Text(hostMessage)
                    .font(.caption)
                    .foregroundStyle(Color.red.opacity(0.92))
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    sourceActionButton("Pick from Photos", action: onPickFromPhotos)
                    sourceActionButton("Use HLS Demo", action: onUseHlsDemo)
                }
            }

            VStack(alignment: .leading, spacing: 10) {
                Text("Theme")
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

            TextField("Remote URL (HLS / progressive)", text: $remoteStreamUrl)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .background(palette.fieldBackground, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                .foregroundStyle(palette.fieldText)

            Button(action: onOpenRemote) {
                Text("Open Remote URL")
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
    private func sourceActionButton(_ title: String, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .buttonStyle(.plain)
            .font(.subheadline.weight(.semibold))
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(.white.opacity(0.08), in: Capsule())
            .foregroundStyle(palette.title)
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
