import SwiftUI
import VesperPlayerKit

struct ExampleSelectionSheetContent: View {
    let sheet: ExamplePlayerSheet
    let uiState: PlayerHostUiState
    let trackCatalog: VesperTrackCatalog
    let trackSelection: VesperTrackSelectionSnapshot
    let effectiveVideoTrackId: String?
    let videoVariantObservation: VesperVideoVariantObservation?
    let fixedTrackStatus: VesperFixedTrackStatus?
    let lastError: VesperPlayerError?
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
                            subtitle: qualityButtonLabel(
                                trackCatalog,
                                trackSelection,
                                effectiveVideoTrackId: effectiveVideoTrackId,
                                fixedTrackStatus: fixedTrackStatus
                            ),
                            selected: false
                        ) {
                            onOpenSheet(.quality)
                        }

                    case .quality:
                        selectionRow(
                            title: ExampleI18n.auto,
                            subtitle: qualityAutoRowSubtitle(
                                trackCatalog,
                                trackSelection,
                                effectiveVideoTrackId: effectiveVideoTrackId,
                                fixedTrackStatus: fixedTrackStatus,
                                videoVariantObservation: videoVariantObservation
                            ),
                            selected: trackSelection.abrPolicy.mode == .auto ||
                                trackSelection.abrPolicy.mode == .constrained
                        ) {
                            onSelectQuality(.auto())
                        }

                        if let notice = qualityRuntimeNotice(lastError) {
                            noticeView(
                                title: ExampleI18n.qualityRuntimeNoticeTitle,
                                message: notice
                            )
                        }

                        let videoTracks = trackCatalog.videoTracks.sorted { left, right in
                            (left.bitRate ?? 0) > (right.bitRate ?? 0)
                        }
                        if videoTracks.isEmpty {
                            emptyState(ExampleI18n.qualityNoVideoTracks)
                        } else {
                            ForEach(videoTracks) { track in
                                selectionRow(
                                    title: qualityLabel(track),
                                    subtitle: qualityOptionSubtitle(
                                        track,
                                        trackCatalog: trackCatalog,
                                        trackSelection: trackSelection,
                                        effectiveVideoTrackId: effectiveVideoTrackId,
                                        fixedTrackStatus: fixedTrackStatus
                                    ),
                                    badge: qualityOptionBadgeLabel(
                                        trackId: track.id,
                                        trackCatalog: trackCatalog,
                                        trackSelection: trackSelection,
                                        effectiveVideoTrackId: effectiveVideoTrackId,
                                        fixedTrackStatus: fixedTrackStatus
                                    ),
                                    selected: trackSelection.abrPolicy.mode == .fixedTrack &&
                                        trackSelection.abrPolicy.trackId == track.id
                                ) {
                                    onSelectQuality(.fixedTrack(track.id))
                                }
                            }
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
        badge: String? = nil,
        selected: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Text(title)
                        .font(.headline.weight(.semibold))
                        .foregroundStyle(.white)
                    if let badge {
                        Text(badge)
                            .font(.caption2.weight(.bold))
                            .foregroundStyle(.white)
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Color.white.opacity(0.14), in: Capsule())
                    }
                }

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

    private func emptyState(_ message: String) -> some View {
        Text(message)
            .font(.footnote)
            .foregroundStyle(Color.white.opacity(0.62))
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
    }

    private func noticeView(title: String, message: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption.weight(.bold))
                .foregroundStyle(.white)
            Text(message)
                .font(.caption)
                .foregroundStyle(Color.white.opacity(0.72))
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}
