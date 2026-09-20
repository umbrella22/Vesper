import XCTest

@MainActor
final class VesperStageSkinInteractionTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--stage-skin-ui-tests"]
        app.launch()
        XCTAssertTrue(app.buttons["standalone-action"].waitForExistence(timeout: 10), app.debugDescription)
    }

    override func tearDownWithError() throws {
        app.terminate()
        app = nil
    }

    func testCustomIconRetainsButtonLabelActionAndMinimumHitArea() {
        let button = app.buttons["standalone-action"]
        XCTAssertEqual(button.label, "Test action")
        XCTAssertGreaterThanOrEqual(button.frame.width, 44)
        XCTAssertGreaterThanOrEqual(button.frame.height, 44)
        XCTAssertFalse(app.buttons["Decorative action"].exists)
        XCTAssertFalse(app.buttons["Decorative icon"].exists)
        button.tap()
        button.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: 2, dy: 22)).tap()
        XCTAssertEqual(app.staticTexts["standalone-actions"].label, "2")
        XCTAssertEqual(app.staticTexts["decorative-actions"].label, "0")
    }

    func testBothLayoutsAndRuntimeSkinSwitchPreserveSurfaceAndActions() {
        let surface = app.descendants(matching: .any)["skin-surface"]
        let initialIdentity = surface.value as? String
        XCTAssertNotNil(initialIdentity)
        for _ in 0..<2 {
            app.buttons["Pause"].tap()
            XCTAssertTrue(app.buttons["Play"].exists)
            app.buttons["Play"].tap()
            app.buttons["Fullscreen"].tap()
            XCTAssertTrue(app.buttons["Exit fullscreen"].exists)
            app.buttons["Exit fullscreen"].tap()
            let beforeSkinChange = app.staticTexts["stage-actions"].label
            for _ in 0..<2 {
                app.buttons["toggle-skin"].tap()
                XCTAssertTrue(app.buttons["Pause"].exists)
                XCTAssertEqual(app.staticTexts["stage-actions"].label, beforeSkinChange)
                XCTAssertEqual(surface.value as? String, initialIdentity)
            }
            app.buttons["toggle-layout"].tap()
        }
        XCTAssertEqual(app.staticTexts["stage-actions"].label, "8")
        XCTAssertEqual(app.staticTexts["decorative-actions"].label, "0")
    }

    func testCustomHUDIconPassesTapThroughToStage() {
        let stage = app.descendants(matching: .any)["skin-surface"]
        let frame = stage.frame
        let origin = app.coordinate(withNormalizedOffset: .zero)
        let start = origin.withOffset(CGVector(dx: frame.minX + 60, dy: frame.midY + 70))
        let end = origin.withOffset(CGVector(dx: frame.minX + 60, dy: frame.midY - 40))
        let hudIcon = origin.withOffset(CGVector(dx: frame.midX - 101.5, dy: frame.midY))
        start.press(forDuration: 0.01, thenDragTo: end, withVelocity: .fast, thenHoldForDuration: 0)
        hudIcon.tap()
        XCTAssertNotEqual(app.staticTexts["brightness-changes"].label, "0")
        XCTAssertEqual(app.staticTexts["decorative-actions"].label, "0")
        XCTAssertEqual(app.staticTexts["hud-touches"].label, "1", "Touch-down must occur inside the visible HUD icon")
        expectation(for: NSPredicate(format: "label == %@", "false"),
                    evaluatedWith: app.staticTexts["controls-visible"])
        waitForExpectations(timeout: 2)
    }
}
