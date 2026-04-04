import SwiftUI
import VesperPlayerKit

struct ExampleSelectionSheetContent: View {
    let sheet: ExamplePlayerSheet
    let uiState: PlayerHostUiState
    let trackCatalog: VesperTrackCatalog
    let trackSelection: VesperTrackSelectionSnapshot
    let onOpenSheet: (ExamplePlayerSheet) -> Void
    let onSelectQuality: (VesperAbrPolicy) -> Void
    let onSelectAudio: (VesperTrackSelection) -> Void
    let onSelectSubtitle: (VesperTrackSelection) -> Void
    let onSelectSpeed: (Float) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text(sheetTitle(sheet))
                    .font(.title2.weight(.bold))
                    .foregroundStyle(.white)

                Text(sheetSubtitle(sheet))
                    .font(.footnote)
                    .foregroundStyle(Color.white.opacity(0.62))
            }
            .padding(.horizontal, 4)
            .padding(.top, 8)
            .padding(.bottom, 2)

            ScrollView {
                VStack(spacing: 6) {
                    switch sheet {
                    case .menu:
                        selectionRow(
                            title: ExampleI18n.playbackSpeed,
                            subtitle: speedBadge(uiState.playbackRate),
                            selected: false
                        ) {
                            onOpenSheet(.speed)
                        }

                        selectionRow(
                            title: ExampleI18n.audio,
                            subtitle: audioButtonLabel(trackCatalog, trackSelection),
                            selected: false
                        ) {
                            onOpenSheet(.audio)
                        }

                        selectionRow(
                            title: ExampleI18n.subtitles,
                            subtitle: subtitleButtonLabel(trackCatalog, trackSelection),
                            selected: false
                        ) {
                            onOpenSheet(.subtitle)
                        }

                        selectionRow(
                            title: ExampleI18n.quality,
                            subtitle: qualityButtonLabel(trackSelection.abrPolicy),
                            selected: false
                        ) {
                            onOpenSheet(.quality)
                        }

                    case .quality:
                        selectionRow(
                            title: ExampleI18n.auto,
                            subtitle: ExampleI18n.qualityAutoSubtitle,
                            selected: trackSelection.abrPolicy.mode == .auto
                        ) {
                            onSelectQuality(.auto())
                        }

                        ForEach(abrPresets()) { preset in
                            selectionRow(
                                title: preset.title,
                                subtitle: preset.subtitle,
                                selected: trackSelection.abrPolicy == preset.policy
                            ) {
                                onSelectQuality(preset.policy)
                            }
                        }

                    case .audio:
                        selectionRow(
                            title: ExampleI18n.auto,
                            subtitle: ExampleI18n.audioAutoSubtitle,
                            selected: trackSelection.audio.mode == .auto
                        ) {
                            onSelectAudio(.auto())
                        }

                        ForEach(trackCatalog.audioTracks) { track in
                            selectionRow(
                                title: audioLabel(track),
                                subtitle: audioSubtitle(track),
                                selected: trackSelection.audio.mode == .track && trackSelection.audio.trackId == track.id
                            ) {
                                onSelectAudio(.track(track.id))
                            }
                        }

                    case .subtitle:
                        selectionRow(
                            title: ExampleI18n.off,
                            subtitle: ExampleI18n.subtitleOffSubtitle,
                            selected: trackSelection.subtitle.mode == .disabled
                        ) {
                            onSelectSubtitle(.disabled())
                        }

                        selectionRow(
                            title: ExampleI18n.auto,
                            subtitle: ExampleI18n.subtitleAutoSubtitle,
                            selected: trackSelection.subtitle.mode == .auto
                        ) {
                            onSelectSubtitle(.auto())
                        }

                        ForEach(trackCatalog.subtitleTracks) { track in
                            selectionRow(
                                title: subtitleLabel(track),
                                subtitle: subtitleSubtitle(track),
                                selected: trackSelection.subtitle.mode == .track && trackSelection.subtitle.trackId == track.id
                            ) {
                                onSelectSubtitle(.track(track.id))
                            }
                        }

                    case .speed:
                        ForEach(VesperPlayerController.supportedPlaybackRates, id: \.self) { rate in
                            selectionRow(
                                title: speedBadge(rate),
                                subtitle: rate == uiState.playbackRate ? ExampleI18n.speedCurrentlyActive : ExampleI18n.speedApplyImmediately,
                                selected: rate == uiState.playbackRate
                            ) {
                                onSelectSpeed(rate)
                            }
                        }
                    }
                }
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color(red: 0.047, green: 0.063, blue: 0.098))
    }

    @ViewBuilder
    private func selectionRow(
        title: String,
        subtitle: String,
        selected: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(.white)

                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(Color.white.opacity(0.62))
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(
                RoundedRectangle(cornerRadius: 18, style: .continuous)
                    .fill(selected ? Color.white.opacity(0.10) : Color.clear)
            )
        }
        .buttonStyle(.plain)

        Divider()
            .overlay(Color.white.opacity(0.04))
    }
}
