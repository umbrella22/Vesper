import UIKit
import XCTest
@testable import VesperPlayerKit

@MainActor
final class VesperScreenSleepRegressionTests: XCTestCase {
    func testUnownedReleasePreservesHostIdleTimerSetting() {
        let previous = UIApplication.shared.isIdleTimerDisabled
        defer { UIApplication.shared.isIdleTimerDisabled = previous }
        UIApplication.shared.isIdleTimerDisabled = true
        VesperScreenSleepCoordinator.release(VesperScreenSleepToken())
        XCTAssertTrue(UIApplication.shared.isIdleTimerDisabled)
    }

    func testOnlyLastOwnerRestoresCapturedSetting() {
        let previous = UIApplication.shared.isIdleTimerDisabled
        defer { UIApplication.shared.isIdleTimerDisabled = previous }
        UIApplication.shared.isIdleTimerDisabled = false
        let first = VesperScreenSleepToken()
        let second = VesperScreenSleepToken()
        VesperScreenSleepCoordinator.setActive(true, for: first)
        VesperScreenSleepCoordinator.setActive(true, for: second)
        VesperScreenSleepCoordinator.release(first)
        XCTAssertTrue(UIApplication.shared.isIdleTimerDisabled)
        VesperScreenSleepCoordinator.release(VesperScreenSleepToken())
        XCTAssertTrue(UIApplication.shared.isIdleTimerDisabled)
        VesperScreenSleepCoordinator.release(second)
        XCTAssertFalse(UIApplication.shared.isIdleTimerDisabled)
    }
}
