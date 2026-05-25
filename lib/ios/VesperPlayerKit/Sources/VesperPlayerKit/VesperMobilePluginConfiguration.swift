import Foundation
import VesperPlayerKitBridgeShim

public enum VesperSourceNormalizerMode: String, Equatable {
    case disabled
    case diagnosticsOnly
    case preflightOnly
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
        }
    }
}

public enum VesperFrameProcessorMode: String, Equatable {
    case disabled
    case diagnosticsOnly
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

private func withOptionalCString<R>(
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

private func withCStringArray<R>(
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
