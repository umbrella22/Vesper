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
                    setSubtitleTrackSelection(publishedTrackSelection.subtitle)
                }
            } else {
                enforceSubtitleVisibility(for: item)
            }
        }
    }

    func enforceSubtitleVisibility(for item: AVPlayerItem) {
        guard !currentSubtitleStyle.visible else { return }
        if let group = subtitleGroup {
            item.select(nil, in: group)
        }
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
