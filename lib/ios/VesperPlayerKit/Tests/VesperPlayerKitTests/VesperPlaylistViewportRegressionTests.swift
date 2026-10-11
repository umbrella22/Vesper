import Foundation
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperPlaylistViewportRegressionTests: XCTestCase {
    func testDuplicateHintsKeepLatestHintInPublicQueueSnapshot() {
        let coordinator = VesperPlaylistCoordinator()
        defer { coordinator.dispose() }
        coordinator.replaceQueue([
            VesperPlaylistQueueItem(itemId: "one", source: .localFile(url: URL(fileURLWithPath: "/tmp/vesper-viewport-fixture.mp4"))),
            VesperPlaylistQueueItem(itemId: "two", source: .localFile(url: URL(fileURLWithPath: "/tmp/vesper-viewport-fixture-2.mp4")))
        ])
        coordinator.updateViewportHints([
            .init(itemId: "one", kind: .nearVisible, order: 0),
            .init(itemId: "one", kind: .visible, order: 1),
            .init(itemId: "missing", kind: .prefetchOnly, order: 2)
        ])
        XCTAssertEqual(coordinator.snapshot.queue.map { $0.viewportHint }, [.visible, .hidden])
        XCTAssertEqual(coordinator.snapshot.queue.map { $0.item.itemId }, ["one", "two"])
    }
}
