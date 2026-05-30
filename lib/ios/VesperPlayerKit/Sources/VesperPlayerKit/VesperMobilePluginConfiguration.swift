import Foundation
import VesperPlayerKitBridgeShim

public enum VesperSourceNormalizerMode: String, Equatable {
    case disabled
    case diagnosticsOnly
    case preflightOnly
    case preferNormalized
    case requireNormalized
}

public struct VesperSourceNormalizerConfiguration: Equatable {
    public let mode: VesperSourceNormalizerMode
    public let pluginLibraryPaths: [String]
    public let runtimeProfile: String?

    public init(
        mode: VesperSourceNormalizerMode = .disabled,
        pluginLibraryPaths: [String] = [],
        runtimeProfile: String? = nil
    ) {
        self.mode = mode
        self.pluginLibraryPaths = pluginLibraryPaths
        self.runtimeProfile = runtimeProfile
    }

    var isDisabled: Bool {
        mode == .disabled && pluginLibraryPaths.isEmpty
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
    public let pluginLibraryPaths: [String]

    public init(
        mode: VesperFrameProcessorMode = .disabled,
        pluginLibraryPaths: [String] = []
    ) {
        self.mode = mode
        self.pluginLibraryPaths = pluginLibraryPaths
    }

    var isDisabled: Bool {
        mode == .disabled && pluginLibraryPaths.isEmpty
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
    public let decoderPluginLibraryPaths: [String]
    public let frameProcessorPluginLibraryPaths: [String]
    public let maxInFlightFrames: Int?

    public init(
        mode: VesperNativeFramePipelineMode = .disabled,
        decoderPluginLibraryPaths: [String] = [],
        frameProcessorPluginLibraryPaths: [String] = [],
        maxInFlightFrames: Int? = nil
    ) {
        self.mode = mode
        self.decoderPluginLibraryPaths = decoderPluginLibraryPaths
        self.frameProcessorPluginLibraryPaths = frameProcessorPluginLibraryPaths
        self.maxInFlightFrames = maxInFlightFrames
    }

    var isDisabled: Bool {
        mode == .disabled &&
            decoderPluginLibraryPaths.isEmpty &&
            frameProcessorPluginLibraryPaths.isEmpty &&
            maxInFlightFrames == nil
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

        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = source.uri.withCString { sourceUriPointer in
            withOptionalCString(sourceNormalizer.runtimeProfile) { runtimeProfilePointer in
                withCStringArray(sourceNormalizer.pluginLibraryPaths) {
                    sourcePathPointers,
                    sourcePathCount in
                    withCStringArray(frameProcessor.pluginLibraryPaths) {
                        framePathPointers,
                        framePathCount in
                        vesper_mobile_plugin_diagnostics_json(
                            sourceUriPointer,
                            sourceNormalizer.ffiMode,
                            sourcePathPointers,
                            UInt(sourcePathCount),
                            runtimeProfilePointer,
                            frameProcessor.ffiMode,
                            framePathPointers,
                            UInt(framePathCount),
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
            return []
        }

        let json = String(cString: outputPointer)
        guard let data = json.data(using: .utf8),
              let records = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else {
            return []
        }
        return records
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

enum VesperMobileSourceNormalizerResource {
    static func open(
        source: VesperPlayerSource,
        configuration: VesperSourceNormalizerConfiguration,
        outputRoot: URL,
        forceNormalized: Bool
    ) -> VesperSourceNormalizerResourceOpenResult? {
        guard configuration.mode == .preferNormalized || configuration.mode == .requireNormalized else {
            return nil
        }

        var handle: UInt64 = 0
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = source.uri.withCString { sourceUriPointer in
            outputRoot.path.withCString { outputRootPointer in
                withOptionalCString(configuration.runtimeProfile) { runtimeProfilePointer in
                    withCStringArray(configuration.pluginLibraryPaths) { pathPointers, pathCount in
                        vesper_source_normalizer_resource_open(
                            sourceUriPointer,
                            configuration.ffiMode,
                            pathPointers,
                            UInt(pathCount),
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
            return nil
        }

        let json = String(cString: outputPointer)
        guard
            let data = json.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
            let route = object["outputRoute"] as? String,
            let primaryPath = object["primaryResourcePath"] as? String
        else {
            vesper_source_normalizer_resource_dispose(handle)
            return nil
        }

        return VesperSourceNormalizerResourceOpenResult(
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
            diagnostics: object["diagnostics"] as? [[String: Any]] ?? []
        )
    }

    static func dispose(handle: UInt64) {
        guard handle != 0 else { return }
        vesper_source_normalizer_resource_dispose(handle)
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

    var duplicated = values.map { strdup($0) }
    defer {
        for pointer in duplicated {
            free(pointer)
        }
    }
    return duplicated.withUnsafeMutableBufferPointer { buffer in
        body(buffer.baseAddress, buffer.count)
    }
}
