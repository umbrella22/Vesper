@testable import VesperPlayerKit
import XCTest

final class VesperPluginReferenceTests: XCTestCase {
    func testReferencePreservesIdentityAndKnownTransport() throws {
        let reference = try VesperPluginReference(
            pluginId: "dev.vesper.example-plugin",
            capabilityInstanceId: "dev.vesper.example-plugin.decoder",
            transport: .native
        )

        XCTAssertEqual(reference.pluginId, "dev.vesper.example-plugin")
        XCTAssertEqual(reference.capabilityInstanceId, "dev.vesper.example-plugin.decoder")
        XCTAssertEqual(reference.transport.rawValue, "native")
    }

    func testDecoderPreservesUnknownTransportWithoutNativeFallback() throws {
        let reference = try VesperPluginReference(
            pluginId: "dev.vesper.example-plugin",
            transportRawValue: "future-sandbox"
        )

        XCTAssertEqual(reference.transport, .unknown("future-sandbox"))
        XCTAssertEqual(reference.transport.rawValue, "future-sandbox")
    }

    func testReferenceRejectsMissingTransportAndLossyIdentityForms() {
        XCTAssertThrowsError(
            try VesperPluginReference(
                pluginId: "dev.vesper.example-plugin",
                transportRawValue: ""
            )
        )
        for invalid in ["Vesper.Plugin", " dev.vesper.plugin ", "dev..plugin", "开发.插件"] {
            XCTAssertThrowsError(
                try VesperPluginReference(pluginId: invalid, transport: .native)
            )
        }
    }

    func testReferenceJSONEncodingPreservesCapabilityAndUnknownTransport() throws {
        let reference = try VesperPluginReference(
            pluginId: "dev.vesper.example-plugin",
            capabilityInstanceId: "dev.vesper.example-plugin.hook",
            transportRawValue: "future-sandbox"
        )

        let json = try encodeVesperPluginReferencesJSON([reference])
        let values = try XCTUnwrap(
            JSONSerialization.jsonObject(with: Data(json.utf8)) as? [[String: String]]
        )

        XCTAssertEqual(values.count, 1)
        XCTAssertEqual(values[0]["pluginId"], "dev.vesper.example-plugin")
        XCTAssertEqual(
            values[0]["capabilityInstanceId"],
            "dev.vesper.example-plugin.hook"
        )
        XCTAssertEqual(values[0]["transport"], "future-sandbox")
        XCTAssertEqual(try encodeVesperPluginReferencesJSON([]), "[]")
    }

    func testDisabledCapabilityModesDoNotActivateStoredPluginReferences() {
        XCTAssertTrue(
            VesperSourceNormalizerConfiguration(
                mode: .disabled,
                pluginReferences: [VesperBundledPluginReferences.sourceNormalizerFfmpeg]
            ).isDisabled
        )
        XCTAssertTrue(
            VesperFrameProcessorConfiguration(
                mode: .disabled,
                pluginReferences: [VesperBundledPluginReferences.frameProcessorDiagnostic]
            ).isDisabled
        )
        XCTAssertTrue(
            VesperNativeFramePipelineConfiguration(
                mode: .disabled,
                decoderPluginReferences: [VesperBundledPluginReferences.decoderVideoToolbox],
                frameProcessorPluginReferences: [VesperBundledPluginReferences.frameProcessorDiagnostic],
                maxInFlightFrames: 3
            ).isDisabled
        )
    }
}
