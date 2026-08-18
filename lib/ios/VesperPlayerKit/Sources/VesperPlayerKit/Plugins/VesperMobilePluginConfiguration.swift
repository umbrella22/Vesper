import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

/// Native playback EventHook plugins selected for this player instance.
///
/// The host resolves references against build-time embedded artifacts before
/// creating the Rust dispatcher.
public struct VesperPipelineEventHookConfiguration: Equatable {
    public let pluginReferences: [VesperPluginReference]

    public init(
        pluginReferences: [VesperPluginReference] = []
    ) {
        self.pluginReferences = pluginReferences
    }

    var isDisabled: Bool {
        pluginReferences.isEmpty
    }
}

public enum VesperSourceNormalizerMode: String, Equatable {
    case disabled
    case diagnosticsOnly
    case preflightOnly
    case preferNormalized
    case requireNormalized
}

public struct VesperSourceNormalizerConfiguration: Equatable {
    public let mode: VesperSourceNormalizerMode
    /// Explicit plugin identities resolved by the build-time host registry.
    public let pluginReferences: [VesperPluginReference]
    public let runtimeProfile: String?

    public init(
        mode: VesperSourceNormalizerMode = .disabled,
        pluginReferences: [VesperPluginReference] = [],
        runtimeProfile: String? = nil
    ) {
        self.mode = mode
        self.pluginReferences = pluginReferences
        self.runtimeProfile = runtimeProfile
    }

    var isDisabled: Bool {
        mode == .disabled
    }

    var supportsPacketInput: Bool {
        switch mode {
        case .preflightOnly, .preferNormalized, .requireNormalized:
            return true
        case .disabled, .diagnosticsOnly:
            return false
        }
    }

    var ffiMode: UInt32 {
        switch mode {
        case .disabled:
            0
        case .diagnosticsOnly:
            1
        case .preflightOnly:
            2
        case .preferNormalized:
            3
        case .requireNormalized:
            4
        }
    }
}

public enum VesperFrameProcessorMode: String, Equatable {
    case disabled
    case diagnosticsOnly
}

public enum VesperNativeFramePipelineMode: String, Equatable {
    case disabled
    case diagnosticsOnly
    case preferNativeFrame
    case requireNativeFrame
}

public struct VesperFrameProcessorConfiguration: Equatable {
    public let mode: VesperFrameProcessorMode
    /// Explicit plugin identities resolved by the build-time host registry.
    public let pluginReferences: [VesperPluginReference]

    public init(
        mode: VesperFrameProcessorMode = .disabled,
        pluginReferences: [VesperPluginReference] = []
    ) {
        self.mode = mode
        self.pluginReferences = pluginReferences
    }

    var isDisabled: Bool {
        mode == .disabled
    }

    var ffiMode: UInt32 {
        switch mode {
        case .disabled:
            0
        case .diagnosticsOnly:
            1
        }
    }
}

public struct VesperNativeFramePipelineConfiguration: Equatable {
    public let mode: VesperNativeFramePipelineMode
    /// Explicit decoder plugin identities resolved by the build-time registry.
    public let decoderPluginReferences: [VesperPluginReference]
    /// Explicit frame processor plugin identities resolved by the build-time registry.
    public let frameProcessorPluginReferences: [VesperPluginReference]
    public let maxInFlightFrames: Int?

    public init(
        mode: VesperNativeFramePipelineMode = .disabled,
        decoderPluginReferences: [VesperPluginReference] = [],
        frameProcessorPluginReferences: [VesperPluginReference] = [],
        maxInFlightFrames: Int? = nil
    ) {
        self.mode = mode
        self.decoderPluginReferences = decoderPluginReferences
        self.frameProcessorPluginReferences = frameProcessorPluginReferences
        self.maxInFlightFrames = maxInFlightFrames
    }

    var isDisabled: Bool {
        mode == .disabled
    }

    var ffiMode: UInt32 {
        switch mode {
        case .disabled:
            0
        case .diagnosticsOnly:
            1
        case .preferNativeFrame:
            2
        case .requireNativeFrame:
            3
        }
    }
}

enum VesperMobilePluginDiagnosticsProbe {
    static func run(
        source: VesperPlayerSource,
        sourceNormalizer: VesperSourceNormalizerConfiguration,
        frameProcessor: VesperFrameProcessorConfiguration
    ) -> [[String: Any]] {
        if sourceNormalizer.isDisabled && frameProcessor.isDisabled {
            return []
        }

        var resolutionDiagnostics: [[String: Any]] = []
        let sourceArtifacts: VesperResolvedPluginArtifacts
        if sourceNormalizer.isDisabled {
            sourceArtifacts = VesperResolvedPluginArtifacts(artifacts: [])
        } else {
            do {
                sourceArtifacts = try VesperBundledPluginResolver.resolvePluginArtifacts(
                    sourceNormalizer.pluginReferences
                )
            } catch {
                sourceArtifacts = VesperResolvedPluginArtifacts(artifacts: [])
                resolutionDiagnostics.append(
                    pluginResolutionDiagnostic(
                        error: error,
                        pluginKind: "source_normalizer",
                        references: sourceNormalizer.pluginReferences
                    )
                )
            }
        }
        let frameArtifacts: VesperResolvedPluginArtifacts
        if frameProcessor.isDisabled {
            frameArtifacts = VesperResolvedPluginArtifacts(artifacts: [])
        } else {
            do {
                frameArtifacts = try VesperBundledPluginResolver.resolvePluginArtifacts(
                    frameProcessor.pluginReferences
                )
            } catch {
                frameArtifacts = VesperResolvedPluginArtifacts(artifacts: [])
                resolutionDiagnostics.append(
                    pluginResolutionDiagnostic(
                        error: error,
                        pluginKind: "frame_processor",
                        references: frameProcessor.pluginReferences
                    )
                )
            }
        }

        // A failed build-time resolution is already a complete diagnostic for
        // this probe. Do not call into the Rust loader with an enabled mode and
        // an empty artifact list: that would turn a deterministic host error
        // into an unbounded/expensive plugin inspection attempt.
        guard resolutionDiagnostics.isEmpty else {
            return resolutionDiagnostics
        }
        let sourceArtifactsJSON: String
        let frameArtifactsJSON: String
        do {
            sourceArtifactsJSON = try encodeVesperResolvedPluginArtifactsJSON(sourceArtifacts)
            frameArtifactsJSON = try encodeVesperResolvedPluginArtifactsJSON(frameArtifacts)
        } catch {
            return [
                pluginResolutionDiagnostic(
                    error: error,
                    pluginKind: "mobile_plugin",
                    references: sourceNormalizer.pluginReferences + frameProcessor.pluginReferences
                )
            ]
        }

        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = source.uri.withCString { sourceUriPointer in
            withOptionalCString(sourceNormalizer.runtimeProfile) { runtimeProfilePointer in
                sourceArtifactsJSON.withCString { sourceArtifactsPointer in
                    frameArtifactsJSON.withCString { frameArtifactsPointer in
                        vesper_mobile_plugin_diagnostics_json(
                            sourceUriPointer,
                            sourceNormalizer.ffiMode,
                            sourceArtifactsPointer,
                            runtimeProfilePointer,
                            frameProcessor.ffiMode,
                            frameArtifactsPointer,
                            &outputPointer,
                            &errorPointer
                        )
                    }
                }
            }
        }
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }

        guard ok, let outputPointer else {
            if let errorPointer {
                iosHostLog("mobile plugin diagnostics failed: \(String(cString: errorPointer))")
            }
            return resolutionDiagnostics
        }

        let json = String(cString: outputPointer)
        guard let data = json.data(using: .utf8),
              let records = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else {
            return []
        }
        return pluginDiagnosticsReplacingArtifactPaths(
            records,
            artifacts: [sourceArtifacts, frameArtifacts]
        ) + resolutionDiagnostics
    }
}

struct VesperSourceNormalizerResourceOpenResult {
    let handle: UInt64
    let outputRoute: String
    let selectedProfile: String?
    let container: String
    let primaryResourcePath: String
    let primaryContentType: String?
    let playbackUri: String?
    let resources: [[String: Any]]
    let cachePolicy: [String: Any]
    let route: String?
    let participation: String?
    let fallbackReason: String?
    let cacheQuota: UInt64?
    let diagnostics: [[String: Any]]

    var playbackURL: URL? {
        if let playbackUri, let url = URL(string: playbackUri) {
            return url
        }
        return URL(fileURLWithPath: primaryResourcePath)
    }
}

struct VesperSourceNormalizerResourceOpenOutcome {
    let resource: VesperSourceNormalizerResourceOpenResult?
    let diagnostics: [[String: Any]]
}

enum VesperMobileSourceNormalizerResource {
    static func open(
        source: VesperPlayerSource,
        configuration: VesperSourceNormalizerConfiguration,
        outputRoot: URL,
        forceNormalized: Bool
    ) -> VesperSourceNormalizerResourceOpenOutcome {
        guard configuration.mode == .preferNormalized || configuration.mode == .requireNormalized else {
            return VesperSourceNormalizerResourceOpenOutcome(resource: nil, diagnostics: [])
        }

        let resolvedArtifacts: VesperResolvedPluginArtifacts
        do {
            resolvedArtifacts = try VesperBundledPluginResolver.resolvePluginArtifacts(
                configuration.pluginReferences
            )
        } catch {
            return VesperSourceNormalizerResourceOpenOutcome(
                resource: nil,
                diagnostics: [
                    pluginResolutionDiagnostic(
                        error: error,
                        pluginKind: "source_normalizer",
                        references: configuration.pluginReferences
                    )
                ]
            )
        }
        let artifactsJSON: String
        do {
            artifactsJSON = try encodeVesperResolvedPluginArtifactsJSON(resolvedArtifacts)
        } catch {
            return VesperSourceNormalizerResourceOpenOutcome(
                resource: nil,
                diagnostics: [
                    pluginResolutionDiagnostic(
                        error: error,
                        pluginKind: "source_normalizer",
                        references: configuration.pluginReferences
                    )
                ]
            )
        }

        var handle: UInt64 = 0
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = source.uri.withCString { sourceUriPointer in
            outputRoot.path.withCString { outputRootPointer in
                withOptionalCString(configuration.runtimeProfile) { runtimeProfilePointer in
                    artifactsJSON.withCString { artifactsPointer in
                        vesper_source_normalizer_resource_open(
                            sourceUriPointer,
                            configuration.ffiMode,
                            artifactsPointer,
                            runtimeProfilePointer,
                            outputRootPointer,
                            forceNormalized,
                            &handle,
                            &outputPointer,
                            &errorPointer
                        )
                    }
                }
            }
        }
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }

        guard ok, handle != 0, let outputPointer else {
            if let errorPointer {
                iosHostLog("source normalizer resource open failed: \(String(cString: errorPointer))")
            }
            return VesperSourceNormalizerResourceOpenOutcome(
                resource: nil,
                diagnostics: pluginDiagnosticsReplacingArtifactPaths(
                    parseDiagnostics(from: outputPointer),
                    artifacts: [resolvedArtifacts]
                )
            )
        }

        let json = String(cString: outputPointer)
        guard
            let data = json.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let route = object["outputRoute"] as? String,
            let primaryPath = object["primaryResourcePath"] as? String
        else {
            vesper_source_normalizer_resource_dispose(handle)
            return VesperSourceNormalizerResourceOpenOutcome(resource: nil, diagnostics: [])
        }

        let diagnostics = pluginDiagnosticsReplacingArtifactPaths(
            object["diagnostics"] as? [[String: Any]] ?? [],
            artifacts: [resolvedArtifacts]
        )
        return VesperSourceNormalizerResourceOpenOutcome(
            resource: VesperSourceNormalizerResourceOpenResult(
                handle: handle,
                outputRoute: route,
                selectedProfile: object["selectedProfile"] as? String,
                container: object["container"] as? String ?? "",
                primaryResourcePath: primaryPath,
                primaryContentType: object["primaryContentType"] as? String,
                playbackUri: object["playbackUri"] as? String,
                resources: object["resources"] as? [[String: Any]] ?? [],
                cachePolicy: object["cachePolicy"] as? [String: Any] ?? [:],
                route: object["route"] as? String,
                participation: object["participation"] as? String,
                fallbackReason: object["fallbackReason"] as? String,
                cacheQuota: (object["cacheQuota"] as? NSNumber)?.uint64Value,
                diagnostics: diagnostics
            ),
            diagnostics: diagnostics
        )
    }

    static func dispose(handle: UInt64) {
        guard handle != 0 else { return }
        vesper_source_normalizer_resource_dispose(handle)
    }

    private static func parseDiagnostics(
        from pointer: UnsafeMutablePointer<CChar>?
    ) -> [[String: Any]] {
        guard let pointer else {
            return []
        }
        let json = String(cString: pointer)
        guard
            let data = json.data(using: .utf8),
            let diagnostics = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else {
            return []
        }
        return diagnostics
    }
}

private func pluginResolutionDiagnostic(
    error: Error,
    pluginKind: String,
    references: [VesperPluginReference]
) -> [String: Any] {
    [
        "pluginId": references.first?.pluginId ?? "unknown",
        "pluginKind": pluginKind,
        "status": "loadFailed",
        "message": error.localizedDescription,
        "participation": "selected",
        "route": "unavailable",
    ]
}

func pluginDiagnosticsReplacingArtifactPaths(
    _ records: [[String: Any]],
    artifacts: [VesperResolvedPluginArtifacts]
) -> [[String: Any]] {
    records.map { record in
        var sanitized = record
        let path = sanitized.removeValue(forKey: "path") as? String
        if let reference = canonicalPluginReferenceObject(
            fromDiagnosticDetails: sanitized["details"]
        ) {
            attachPluginReference(reference, to: &sanitized)
            return sanitized
        }
        guard let path else {
            return sanitized
        }

        var references: [VesperPluginReference] = []
        for reference in artifacts.flatMap({ $0.references(forLibraryPath: path) })
        where !references.contains(reference) {
            references.append(reference)
        }
        if references.count == 1, let reference = references.first {
            attachPluginReference(vesperPluginReferenceJSONObject(reference), to: &sanitized)
        } else if !references.isEmpty {
            sanitized["pluginReferences"] = references.map(vesperPluginReferenceJSONObject)
            let pluginIds = Set(references.map(\.pluginId))
            if pluginIds.count == 1 {
                sanitized["pluginId"] = pluginIds.first
            }
        }
        return sanitized
    }
}

private func canonicalPluginReferenceObject(
    fromDiagnosticDetails value: Any?
) -> [String: Any]? {
    guard
        let details = value as? [String: Any],
        let pluginId = details["pluginId"] as? String,
        let transport = details["transport"] as? String
    else {
        return nil
    }
    var reference: [String: Any] = [
        "pluginId": pluginId,
        "transport": transport,
    ]
    if let instanceId = details["capabilityInstanceId"] as? String {
        reference["capabilityInstanceId"] = instanceId
    }
    return reference
}

private func attachPluginReference(
    _ reference: [String: Any],
    to diagnostic: inout [String: Any]
) {
    diagnostic["pluginReference"] = reference
    diagnostic["pluginId"] = reference["pluginId"]
    diagnostic["transport"] = reference["transport"]
    if let instanceId = reference["capabilityInstanceId"] {
        diagnostic["capabilityInstanceId"] = instanceId
    }
}

func withOptionalCString<R>(
    _ value: String?,
    _ body: (UnsafePointer<CChar>?) -> R
) -> R {
    guard let value else {
        return body(nil)
    }
    return value.withCString { pointer in
        body(pointer)
    }
}

func withCStringArray<R>(
    _ values: [String],
    _ body: (UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?, Int) -> R
) -> R {
    if values.isEmpty {
        return body(nil, 0)
    }

    var duplicated: [UnsafeMutablePointer<CChar>?] = []
    duplicated.reserveCapacity(values.count)
    for value in values {
        guard let dup = strdup(value) else {
            for ptr in duplicated {
                free(ptr)
            }
            return body(nil, 0)
        }
        duplicated.append(dup)
    }
    defer {
        for pointer in duplicated {
            free(pointer)
        }
    }
    return duplicated.withUnsafeMutableBufferPointer { buffer in
        body(buffer.baseAddress, buffer.count)
    }
}
