@testable import VesperPlayerKit
import XCTest

final class VesperPipelineEventHookReportsTests: XCTestCase {
    func testDecodesTypedOutcomeDiagnosticsAndUnknownWireValues() throws {
        let data = try XCTUnwrap(
            """
            {
              "reports":[{
                "pluginId":"dev.vesper.hook",
                "capabilityInstanceId":"dev.vesper.hook.playback",
                "transport":"future-transport",
                "runId":"run-1",
                "sessionId":"session-1",
                "eventName":"playback.ready",
                "result":{
                  "status":"future-status",
                  "outcome":{
                    "accepted":true,
                    "measurements":[{"name":"latency","value":2.5,"unit":"ms","attributes":{"stage":"open"}}],
                    "diagnostics":[{"code":"future.code","severity":"future-severity","message":"ready","attributes":{"scope":"test"}}]
                  },
                  "error":null
                }
              }],
              "droppedEvents":3,
              "droppedReports":2,
              "dispatcherError":null
            }
            """.data(using: .utf8)
        )

        let batch = decodeVesperPipelineEventHookReportBatch(data: data, bridgeError: nil)

        let report = try XCTUnwrap(batch.reports.first)
        XCTAssertEqual(report.pluginId, "dev.vesper.hook")
        XCTAssertEqual(report.transport.rawValue, "future-transport")
        XCTAssertEqual(report.result.status.rawValue, "future-status")
        XCTAssertEqual(report.result.outcome?.measurements.first?.value, 2.5)
        XCTAssertEqual(
            report.result.outcome?.diagnostics.first?.severity.rawValue,
            "future-severity"
        )
        XCTAssertEqual(batch.droppedEvents, 3)
        XCTAssertEqual(batch.droppedReports, 2)
        XCTAssertNil(batch.dispatcherError)
    }

    func testDecodesErrorResultWithUnknownErrorCode() throws {
        let data = try XCTUnwrap(
            """
            {"reports":[{"pluginId":"dev.vesper.hook","transport":"native","runId":"run","sessionId":"session","eventName":"failed","result":{"status":"error","outcome":null,"error":{"code":"future-error","message":"failed"}}}]}
            """.data(using: .utf8)
        )

        let batch = decodeVesperPipelineEventHookReportBatch(data: data, bridgeError: nil)

        XCTAssertEqual(batch.reports.first?.result.error?.code.rawValue, "future-error")
        XCTAssertEqual(batch.reports.first?.result.error?.message, "failed")
    }

    func testPreservesBridgeErrorForMalformedPayload() {
        let batch = decodeVesperPipelineEventHookReportBatch(
            data: Data("not-json".utf8),
            bridgeError: "native drain failed"
        )

        XCTAssertTrue(batch.reports.isEmpty)
        XCTAssertEqual(batch.dispatcherError, "native drain failed")
    }

    func testRejectsNegativeCountersAndNonObjectReports() throws {
        let data = try XCTUnwrap(
            """
            {"reports":[1],"droppedEvents":-1}
            """.data(using: .utf8)
        )

        let batch = decodeVesperPipelineEventHookReportBatch(data: data, bridgeError: nil)

        XCTAssertEqual(batch.dispatcherError, "playback EventHook report entry was not an object")
        XCTAssertTrue(batch.reports.isEmpty)
    }

    func testNamesMalformedRequiredFieldInDispatcherError() throws {
        let data = try XCTUnwrap(
            """
            {"reports":[{"pluginId":"dev.vesper.hook","transport":3,"runId":"run","sessionId":"session","eventName":"ready","result":{"status":"accepted","outcome":{"accepted":true}}}]}
            """.data(using: .utf8)
        )

        let batch = decodeVesperPipelineEventHookReportBatch(data: data, bridgeError: nil)

        XCTAssertEqual(
            batch.dispatcherError,
            "playback EventHook transport was missing or empty"
        )
        XCTAssertTrue(batch.reports.isEmpty)
    }

    func testRejectsProtocolLimitOverflow() throws {
        let measurements = String(
            repeating: "{\"name\":\"m\",\"value\":1,\"unit\":\"ms\"},",
            count: 128
        ) + "{\"name\":\"m\",\"value\":1,\"unit\":\"ms\"}"
        let data = try XCTUnwrap(
            """
            {"reports":[{"pluginId":"dev.vesper.hook","transport":"native","runId":"run","sessionId":"session","eventName":"overflow","result":{"status":"accepted","outcome":{"accepted":true,"measurements":[\(measurements)]}}}]}
            """.data(using: .utf8)
        )

        let batch = decodeVesperPipelineEventHookReportBatch(data: data, bridgeError: nil)

        XCTAssertEqual(
            batch.dispatcherError,
            "playback EventHook outcome exceeds the 128-measurement limit"
        )
        XCTAssertTrue(batch.reports.isEmpty)
    }

    func testMapsTypedReportsForHostBridges() throws {
        let report = VesperPipelineEventHookReport(
            pluginId: "dev.vesper.hook",
            capabilityInstanceId: nil,
            transport: .wasm,
            runId: "run",
            sessionId: "session",
            eventName: "ready",
            result: VesperPipelineEventHookResult(
                status: .accepted,
                outcome: VesperPipelineEventHookOutcome(accepted: true)
            )
        )
        let map = VesperPipelineEventHookReportBatch(reports: [report]).toMap()
        let reports = try XCTUnwrap(map["reports"] as? [[String: Any]])
        let result = try XCTUnwrap(reports.first?["result"] as? [String: Any])

        XCTAssertEqual(result["status"] as? String, "accepted")
        XCTAssertTrue(result["outcome"] is [String: Any])
        XCTAssertTrue(result["error"] is NSNull)
        XCTAssertEqual(map["droppedEvents"] as? UInt64, 0)
        XCTAssertEqual(map["droppedReports"] as? UInt64, 0)
        XCTAssertTrue(map["dispatcherError"] is NSNull)
    }
}
