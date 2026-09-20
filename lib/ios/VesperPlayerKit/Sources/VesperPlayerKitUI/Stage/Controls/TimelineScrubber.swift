import SwiftUI
import VesperPlayerKit
struct TimelineScrubber: View {
    @Environment(\.vesperPlayerStageSkin) private var skin
    private var knobSize: CGFloat { compact ? skin.metrics.timelineThumbSize : skin.metrics.timelineLargeThumbSize }
    private var touchHeight: CGFloat { max(compact ? 22 : 28, max(knobSize, skin.metrics.timelineTrackHeight) + 8) }
    let displayedRatio: Double
    let compact: Bool
    var enabled: Bool = true
    let onSeekPreview: (Double) -> Void
    let onSeekCommit: (Double) -> Void
    let onSeekCancel: () -> Void

    var body: some View {
        GeometryReader { proxy in
            let width = max(proxy.size.width, 1)
            let ratio = displayedRatio.clamped(to: 0...1)
            let knobOffset = max(0, min(width - knobSize, width * ratio - knobSize / 2))
            let activeOpacity = enabled ? 1.0 : 0.42

            ZStack(alignment: .leading) {
                Capsule()
                    .fill(skin.colors.timelineInactive.opacity(enabled ? 0.16 : 0.10))
                    .frame(height: skin.metrics.timelineTrackHeight)

                Capsule()
                    .fill(
                        LinearGradient(
                            colors: [
                                skin.colors.timelineStart,
                                skin.colors.timelineEnd,
                            ],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .opacity(activeOpacity)
                    .frame(width: width * ratio, height: skin.metrics.timelineTrackHeight)

                Circle()
                    .fill(skin.colors.timelineThumb.opacity(activeOpacity))
                    .frame(width: knobSize, height: knobSize)
                    .offset(x: knobOffset)
            }
            .frame(height: touchHeight, alignment: .center)
            .contentShape(Rectangle())
            .allowsHitTesting(enabled)
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { value in
                        onSeekPreview((value.location.x / width).clamped(to: 0...1))
                    }
                    .onEnded { value in
                        onSeekCommit((value.location.x / width).clamped(to: 0...1))
                    }
            )
        }
        .frame(height: touchHeight)
        .accessibilityRepresentation {
            // Use the system slider's adjustable actions and disabled semantics
            // while retaining the custom track and touch gestures above.
            Slider(
                value: Binding(
                    get: { displayedRatio.clamped(to: 0...1) },
                    set: { ratio in
                        guard enabled else { return }
                        let target = ratio.clamped(to: 0...1)
                        guard target != displayedRatio.clamped(to: 0...1) else { return }
                        onSeekPreview(target)
                        onSeekCommit(target)
                    }
                ),
                in: 0...1,
                step: 0.05
            ) {
                Text(VesperPlayerStageStrings.playbackPosition)
            }
            .disabled(!enabled)
            .accessibilityValue(
                Text(displayedRatio.clamped(to: 0...1), format: .percent.precision(.fractionLength(0)))
            )
        }
    }
}
