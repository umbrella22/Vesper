@preconcurrency import AVFoundation
import CoreMedia
import Foundation

extension VesperNativePlayerBridge {
    func setSubtitleStyle(_ style: VesperSubtitleStyle) {
        guard style.fontScale.isFinite, (0.5...3.0).contains(style.fontScale) else {
            reportCommandError(
                code: .unsupported,
                category: .capability,
                message: "Subtitle fontScale must be finite and between 0.5 and 3.0."
            )
            return
        }
        clearLastError()
        let wasVisible = currentSubtitleStyle.visible
        currentSubtitleStyle = style
        subtitleOverlayRenderer.setStyle(style)
        if let item = player?.currentItem {
            applySubtitleStyle(style, to: item)
            if style.visible {
                if !wasVisible {
                    // Style toggles are best-effort: if the previous
                    // subtitle selection is no longer valid (e.g. source
                    // switched underneath), don't fail the style call.
                    let selection = confirmedSubtitleSelection
                    Task { @MainActor [weak self] in
                        guard let self else { return }
                        do {
                            try await self.coordinateSubtitleSelection(
                                selection,
                                origin: .visibilityRestore
                            )
                        } catch {
                            iosHostLog(
                                "subtitle visibility restore failed: \(error.localizedDescription)"
                            )
                        }
                    }
                }
            } else {
                enforceSubtitleVisibility(for: item)
            }
        }
    }

    func enforceSubtitleVisibility(for item: AVPlayerItem) {
        // Visibility is a rendering concern. Keep AVPlayer's selected option
        // intact so the confirmed/effective selection remains truthful; the
        // text style rule below hides native cues without changing selection.
        applySubtitleStyle(currentSubtitleStyle, to: item)
    }

    func applySubtitleStyle(_ style: VesperSubtitleStyle, to item: AVPlayerItem) {
        let foregroundAlpha: NSNumber = style.visible ? 1.0 : 0.0
        let attributes: [String: Any] = [
            kCMTextMarkupAttribute_RelativeFontSize as String: NSNumber(
                value: Double(style.fontScale * 100)
            ),
            kCMTextMarkupAttribute_ForegroundColorARGB as String: [
                foregroundAlpha,
                NSNumber(value: 1.0),
                NSNumber(value: 1.0),
                NSNumber(value: 1.0),
            ],
        ]
        item.textStyleRules = AVTextStyleRule(textMarkupAttributes: attributes).map { [$0] }
    }
}
