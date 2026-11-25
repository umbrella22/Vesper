import SwiftUI

/// "Open Media" sheet: Photos picker, stream URL, and quick samples.
struct ExampleOpenMediaSheet: View {
    @Binding var remoteStreamUrl: String
    let hostMessage: String?
    let onPickVideo: () -> Void
    let onOpenRemote: () -> Void
    let onSelectSample: (String) -> Void
    let onDismiss: () -> Void

    var body: some View {
        NavigationStack {
            List {
                Section {
                    Button(action: onPickVideo) {
                        ExampleShellRowLabel(
                            title: ExampleI18n.pickFromPhotos,
                            subtitle: ExampleI18n.openMediaPhotosSubtitle,
                            systemImage: "photo.on.rectangle"
                        )
                    }
                    .accessibilityIdentifier("open-media.photos")
                }

                Section {
                    TextField(ExampleI18n.remoteUrlPlaceholder, text: $remoteStreamUrl)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .submitLabel(.go)
                        .onSubmit(onOpenRemote)

                    Button(action: onOpenRemote) {
                        Text(ExampleI18n.openRemoteUrl)
                            .font(.headline)
                            .frame(maxWidth: .infinity)
                            .padding(.vertical, 4)
                    }
                    .exampleGlassButtonStyle(prominent: true)
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets(top: 6, leading: 0, bottom: 6, trailing: 0))
                    .accessibilityIdentifier("open-media.open-remote")
                } header: {
                    Text(ExampleI18n.openMediaStreamUrl)
                } footer: {
                    if let hostMessage {
                        Text(hostMessage).foregroundStyle(.red)
                    } else {
                        Text(ExampleI18n.openMediaStreamUrlHint)
                    }
                }

                Section {
                    ForEach(exampleOpenMediaQuickSamples()) { sample in
                        Button {
                            onSelectSample(sample.id)
                        } label: {
                            ExampleShellRowLabel(
                                title: sample.title,
                                subtitle: sample.subtitle,
                                systemImage: "play.circle"
                            )
                        }
                    }
                } header: {
                    Text(ExampleI18n.openMediaQuickSamples)
                } footer: {
                    Text(ExampleI18n.openMediaDashBridgeNote)
                        .padding(.bottom, 12)
                }
            }
            .navigationTitle(ExampleI18n.openMediaTitle)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(ExampleI18n.shellCancel, action: onDismiss)
                }
            }
        }
    }
}

/// Row label used by shell lists. Titles may wrap to two lines instead of
/// truncating long names such as "Live DVR Acceptance (Unified SCTE-35)".
struct ExampleShellRowLabel: View {
    let title: String
    let subtitle: String?
    let systemImage: String
    /// Short trailing value (for example an item count) that must never be
    /// truncated by a long subtitle.
    var trailingText: String? = nil
    var showsChevron = false

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: systemImage)
                .font(.title3)
                .foregroundStyle(.secondary)
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
                if let subtitle, !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            Spacer(minLength: 8)
            if let trailingText, !trailingText.isEmpty {
                Text(trailingText)
                    .font(.footnote.monospacedDigit())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .fixedSize()
                    .layoutPriority(1)
            }
            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.footnote.weight(.semibold))
                    .foregroundStyle(.tertiary)
            }
        }
        .contentShape(Rectangle())
    }
}
