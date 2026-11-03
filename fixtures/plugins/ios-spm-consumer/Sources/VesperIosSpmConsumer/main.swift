import Foundation
import VesperPlayerKit

@MainActor
func makeConsumerController() -> VesperPlayerController {
    VesperPlayerControllerFactory.makeDefault(
        initialSource: VesperPlayerSource.hls(
            url: URL(string: "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8")!,
            label: "Hosted SwiftPM consumer"
        )
    )
}

_ = makeConsumerController
