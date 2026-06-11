import SwiftUI
import VesperPlayerKit
struct TimelineScrubber: View {
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
            let knobSize = compact ? 12.0 : 14.0
            let knobOffset = max(0, min(width - knobSize, width * ratio - knobSize / 2))
            let activeOpacity = enabled ? 1.0 : 0.42

            ZStack(alignment: .leading) {
                Capsule()
                    .fill(Color.white.opacity(enabled ? 0.16 : 0.10))
                    .frame(height: 4)

                Capsule()
                    .fill(
                        LinearGradient(
                            colors: [
                                Color(red: 1.0, green: 0.42, blue: 0.56),
                                Color(red: 1.0, green: 0.71, blue: 0.33),
                            ],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
                    .opacity(activeOpacity)
                    .frame(width: width * ratio, height: 4)

                Circle()
                    .fill(Color.white.opacity(activeOpacity))
                    .frame(width: knobSize, height: knobSize)
                    .offset(x: knobOffset)
            }
            .frame(height: compact ? 22 : 28, alignment: .center)
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
        .frame(height: compact ? 22 : 28)
    }
}
