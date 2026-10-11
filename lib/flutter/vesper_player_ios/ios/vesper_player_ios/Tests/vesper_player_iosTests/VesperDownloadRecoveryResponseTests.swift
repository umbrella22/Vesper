import Flutter
import Foundation
import VesperPlayerKit
import XCTest
@testable import vesper_player_ios

@MainActor
final class VesperDownloadRecoveryResponseTests: XCTestCase {
    private var validPlan: [String: Any] {
        [
            "source": ["source": ["uri": "https://fixture.test/index.mpd", "protocol": "dash", "kind": "remote"]],
            "profile": [:],
            "assetIndex": ["resources": [["resourceId": "manifest", "uri": "https://fixture.test/index.mpd"]]],
        ]
    }

    func testMalformedRecoveryReturnsErrorImmediatelyInsteadOfNilOrTimeout() async {
        var malformed = validPlan
        malformed["assetIndex"] = ["resources": [["resourceId": "manifest", "uri": "https://fixture.test"]],
                                   "segments": [NSNull()]]
        for bad: Any in ["not a plan", malformed] {
            do {
                _ = try await awaitDownloadRecoveryResponse {
                    $0(bad)
                }
                XCTFail("Malformed recovery must throw")
            } catch {
                XCTAssertTrue(error is PluginError)
            }
        }
    }

    func testValidAndDeclinedRecoveryRemainDistinct() async throws {
        let valid = try await awaitDownloadRecoveryResponse { $0(validPlan) }
        XCTAssertEqual(valid?.assetIndex.resources.first?.resourceId, "manifest")
        for response: Any? in [nil, NSNull(), FlutterMethodNotImplemented] {
            let declined = try await awaitDownloadRecoveryResponse { $0(response) }
            XCTAssertNil(declined)
        }
    }

    func testFirstMalformedCallbackWinsOverDuplicateSuccess() async {
        do {
            _ = try await awaitDownloadRecoveryResponse { callback in
                callback("malformed")
                callback(validPlan)
                callback(nil)
            }
            XCTFail("The first failure must not be overwritten")
        } catch { XCTAssertTrue(error is PluginError) }
    }

    func testFirstValidCallbackWinsOverDuplicateFailure() async throws {
        let result = try await awaitDownloadRecoveryResponse { callback in
            callback(validPlan)
            callback("malformed")
        }
        XCTAssertEqual(result?.assetIndex.resources.first?.resourceId, "manifest")
    }

    func testCancellationBeforeActivationPreservesFailureAndDoesNotInvokeChannel() async {
        var invocations = 0
        let task = Task { @MainActor in
            withUnsafeCurrentTask { $0?.cancel() }
            do {
                _ = try await awaitDownloadRecoveryResponse { _ in invocations += 1 }
                XCTFail("Cancellation must throw")
            } catch is CancellationError {
                // Expected control flow.
            } catch { XCTFail("Unexpected error: \(error)") }
        }
        await task.value
        XCTAssertEqual(invocations, 0)
    }

    func testCancellationDuringCallbackWaitCompletesOnceWithoutTimeout() async {
        let invoked = expectation(description: "Recovery channel invoked")
        var callback: FlutterResult?
        let task = Task { @MainActor in
            do {
                _ = try await awaitDownloadRecoveryResponse { completion in
                    callback = completion
                    invoked.fulfill()
                }
                XCTFail("Cancellation must throw")
            } catch is CancellationError {
                // Expected control flow.
            } catch { XCTFail("Unexpected error: \(error)") }
        }
        await fulfillment(of: [invoked], timeout: 2)
        task.cancel()
        await task.value
        callback?(validPlan)
        callback?("late invalid response")
    }

    func testTimeoutStillDeclinesAndLateCallbackCannotResumeTwice() async throws {
        var callback: FlutterResult?
        let result = try await awaitDownloadRecoveryResponse(timeoutNanoseconds: 1_000_000) { callback = $0 }
        XCTAssertNil(result)
        callback?(validPlan)
        callback?("late invalid response")
    }

    func testChannelFailureIsReturnedAsAnError() async {
        do {
            _ = try await awaitDownloadRecoveryResponse {
                $0(FlutterError(code: "provider_failed", message: "provider failure", details: nil))
            }
            XCTFail("Channel failures must propagate")
        } catch {
            XCTAssertTrue(error.localizedDescription.contains("provider_failed"))
            XCTAssertTrue(error.localizedDescription.contains("provider failure"))
        }
    }

    func testCompletionBeforeContinuationActivationRetainsOriginalFailure() async {
        let box = DownloadRecoveryContinuationBox()
        box.resume(with: .failure(PluginError.invalidSource("early failure")))
        do {
            let _: VesperDownloadRecoveredTaskPlan? = try await withCheckedThrowingContinuation { box.activate($0) }
            XCTFail("Early failure must not become a nil plan")
        } catch { XCTAssertEqual(error.localizedDescription, "early failure") }
        box.resume(with: .success(nil))
    }
}
