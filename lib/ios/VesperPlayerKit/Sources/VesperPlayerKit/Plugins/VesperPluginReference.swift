import Foundation

public enum VesperPluginTransport: Equatable, Sendable {
    case native
    case wasm
    case unknown(String)

    public init(rawValue: String) {
        switch rawValue {
        case "native":
            self = .native
        case "wasm":
            self = .wasm
        default:
            self = .unknown(rawValue)
        }
    }

    public var rawValue: String {
        switch self {
        case .native:
            "native"
        case .wasm:
            "wasm"
        case let .unknown(rawValue):
            rawValue
        }
    }
}

public enum VesperPluginReferenceError: Error, Equatable {
    case invalidPluginId
    case invalidCapabilityInstanceId
    case missingTransport
}

enum VesperPluginReferenceEncodingError: LocalizedError {
    case invalidUTF8

    var errorDescription: String? {
        switch self {
        case .invalidUTF8:
            "Plugin references could not be encoded as UTF-8 JSON."
        }
    }
}

/// Explicit selection of one plugin transport and optional capability instance.
public struct VesperPluginReference: Equatable, Sendable {
    public let pluginId: String
    public let capabilityInstanceId: String?
    public let transport: VesperPluginTransport

    public init(
        pluginId: String,
        capabilityInstanceId: String? = nil,
        transport: VesperPluginTransport
    ) throws {
        guard isValidPluginIdentity(pluginId) else {
            throw VesperPluginReferenceError.invalidPluginId
        }
        if let capabilityInstanceId, !isValidPluginIdentity(capabilityInstanceId) {
            throw VesperPluginReferenceError.invalidCapabilityInstanceId
        }
        if case let .unknown(rawValue) = transport, rawValue.isEmpty {
            throw VesperPluginReferenceError.missingTransport
        }
        self.pluginId = pluginId
        self.capabilityInstanceId = capabilityInstanceId
        self.transport = transport
    }

    public init(
        pluginId: String,
        capabilityInstanceId: String? = nil,
        transportRawValue: String
    ) throws {
        guard !transportRawValue.isEmpty else {
            throw VesperPluginReferenceError.missingTransport
        }
        try self.init(
            pluginId: pluginId,
            capabilityInstanceId: capabilityInstanceId,
            transport: VesperPluginTransport(rawValue: transportRawValue)
        )
    }

    fileprivate init(
        knownValidPluginId pluginId: String,
        capabilityInstanceId: String? = nil
    ) {
        self.pluginId = pluginId
        self.capabilityInstanceId = capabilityInstanceId
        self.transport = .native
    }
}

/// Canonical references for plugins distributed with Vesper iOS host kits.
public enum VesperBundledPluginReferences {
    public static let sourceNormalizerFfmpeg = VesperPluginReference(
        knownValidPluginId: "io.github.umbrella22.vesper.source-normalizer-ffmpeg"
    )
    public static let remuxFfmpeg = VesperPluginReference(
        knownValidPluginId: "io.github.umbrella22.vesper.remux-ffmpeg"
    )
    public static let decoderVideoToolbox = VesperPluginReference(
        knownValidPluginId: "io.github.umbrella22.vesper.decoder-videotoolbox"
    )
    public static let frameProcessorDiagnostic = VesperPluginReference(
        knownValidPluginId: "dev.vesper.frame-processor-diagnostic"
    )
    public static let performanceDiagnostics = VesperPluginReference(
        knownValidPluginId: "io.github.umbrella22.vesper.performance-diagnostics",
        capabilityInstanceId:
            "io.github.umbrella22.vesper.performance-diagnostics.benchmark"
    )
}

private func isValidPluginIdentity(_ value: String) -> Bool {
    guard
        !value.isEmpty,
        value.utf8.count <= 255,
        value.unicodeScalars.allSatisfy({ $0.value <= 0x7f })
    else {
        return false
    }
    let segments = value.split(separator: ".", omittingEmptySubsequences: false)
    return segments.count >= 2 && segments.allSatisfy(isValidPluginIdentitySegment)
}

private func isValidPluginIdentitySegment(_ segment: Substring) -> Bool {
    guard
        let first = segment.utf8.first,
        let last = segment.utf8.last,
        first >= 0x61, first <= 0x7a,
        (last >= 0x61 && last <= 0x7a) || (last >= 0x30 && last <= 0x39)
    else {
        return false
    }
    return segment.utf8.allSatisfy { byte in
        (byte >= 0x61 && byte <= 0x7a) ||
            (byte >= 0x30 && byte <= 0x39) ||
            byte == 0x2d
    }
}

func encodeVesperPluginReferencesJSON(
    _ references: [VesperPluginReference]
) throws -> String {
    let values = references.map(vesperPluginReferenceJSONObject)
    return try encodeVesperPluginJSONObject(values)
}

func encodeVesperResolvedPluginArtifactsJSON(
    _ artifacts: VesperResolvedPluginArtifacts
) throws -> String {
    guard artifacts.artifacts.count <= 256 else {
        throw VesperBundledPluginResolutionError.tooManyReferences(artifacts.artifacts.count)
    }
    let values = artifacts.artifacts.map { artifact in
        [
            "reference": vesperPluginReferenceJSONObject(artifact.reference),
            "libraryPath": artifact.libraryPath,
        ] as [String: Any]
    }
    return try encodeVesperPluginJSONObject(values)
}

func vesperPluginReferenceJSONObject(
    _ reference: VesperPluginReference
) -> [String: Any] {
    var value: [String: Any] = [
        "pluginId": reference.pluginId,
        "transport": reference.transport.rawValue,
    ]
    if let capabilityInstanceId = reference.capabilityInstanceId {
        value["capabilityInstanceId"] = capabilityInstanceId
    }
    return value
}

private func encodeVesperPluginJSONObject(_ values: Any) throws -> String {
    let data = try JSONSerialization.data(withJSONObject: values, options: [.sortedKeys])
    guard let json = String(data: data, encoding: .utf8) else {
        throw VesperPluginReferenceEncodingError.invalidUTF8
    }
    return json
}
