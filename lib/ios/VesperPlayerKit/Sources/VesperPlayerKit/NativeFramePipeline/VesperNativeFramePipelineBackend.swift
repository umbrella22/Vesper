@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import VesperPlayerKitBridgeShim

struct VesperNativeFramePipelineOpenResult {
    let handle: UInt64
    let status: [String: Any]
}

struct VesperNativeFramePipelineOperationError: LocalizedError, Equatable {
    let message: String

    var errorDescription: String? {
        message
    }
}

protocol VesperNativeFramePipelineBackend: AnyObject, Sendable {
    func open(
        source: VesperPlayerSource,
        configuration: VesperNativeFramePipelineConfiguration,
        sourceNormalizer: VesperSourceNormalizerConfiguration
    ) -> Result<VesperNativeFramePipelineOpenResult, VesperNativeFramePipelineStartupError>

    func flush(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func seek(
        handle: UInt64,
        positionMs: Int64
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func advance(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func releaseFrame(
        handle: UInt64,
        frameHandle: UInt64,
        presented: Bool
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError>

    func close(handle: UInt64)
}

final class VesperFfiNativeFramePipelineBackend: VesperNativeFramePipelineBackend, @unchecked Sendable {
    func open(
        source: VesperPlayerSource,
        configuration: VesperNativeFramePipelineConfiguration,
        sourceNormalizer: VesperSourceNormalizerConfiguration
    ) -> Result<VesperNativeFramePipelineOpenResult, VesperNativeFramePipelineStartupError> {
        var openedHandle: UInt64 = 0
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let maxInFlightFrames = UInt32(configuration.maxInFlightFrames ?? 0)
        let ok = source.uri.withCString { sourceUriPointer in
            withOptionalCString(sourceNormalizer.runtimeProfile) { runtimeProfilePointer in
                withCStringArray(sourceNormalizer.pluginLibraryPaths) {
                    sourcePathPointers,
                    sourcePathCount in
                    withCStringArray(configuration.decoderPluginLibraryPaths) {
                        decoderPathPointers,
                        decoderPathCount in
                        withCStringArray(configuration.frameProcessorPluginLibraryPaths) {
                            framePathPointers,
                            framePathCount in
                            vesper_ios_native_frame_pipeline_open(
                                sourceUriPointer,
                                sourceNormalizer.ffiMode,
                                sourcePathPointers,
                                UInt(sourcePathCount),
                                runtimeProfilePointer,
                                configuration.ffiMode,
                                decoderPathPointers,
                                UInt(decoderPathCount),
                                framePathPointers,
                                UInt(framePathCount),
                                maxInFlightFrames,
                                &openedHandle,
                                &outputPointer,
                                &errorPointer
                            )
                        }
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

        guard ok, openedHandle != 0, let outputPointer else {
            let message = errorPointer.map { String(cString: $0) }
                ?? "iOS native-frame pipeline open failed."
            return .failure(
                VesperNativeFramePipelineStartupError(
                    issue: VesperNativeFramePipelineIssue.classifyStartupFailure(message)
                )
            )
        }

        return .success(
            VesperNativeFramePipelineOpenResult(
                handle: openedHandle,
                status: Self.jsonObject(from: outputPointer)
            )
        )
    }

    func flush(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_flush(
            handle,
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame flush failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func seek(
        handle: UInt64,
        positionMs: Int64
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_seek(
            handle,
            UInt64(max(positionMs, 0)),
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame seek failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func advance(handle: UInt64) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_advance(
            handle,
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame advance failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func releaseFrame(
        handle: UInt64,
        frameHandle: UInt64,
        presented: Bool
    ) -> Result<[String: Any], VesperNativeFramePipelineOperationError> {
        var outputPointer: UnsafeMutablePointer<CChar>?
        var errorPointer: UnsafeMutablePointer<CChar>?
        let ok = vesper_ios_native_frame_pipeline_release_frame(
            handle,
            frameHandle,
            presented,
            &outputPointer,
            &errorPointer
        )
        defer {
            if let outputPointer {
                vesper_mobile_plugin_diagnostics_string_free(outputPointer)
            }
            if let errorPointer {
                vesper_mobile_plugin_diagnostics_string_free(errorPointer)
            }
        }
        guard ok, let outputPointer else {
            return .failure(
                VesperNativeFramePipelineOperationError(
                    message: errorPointer.map { String(cString: $0) }
                        ?? "native-frame release failed"
                )
            )
        }
        return .success(Self.jsonObject(from: outputPointer))
    }

    func close(handle: UInt64) {
        vesper_ios_native_frame_pipeline_close(handle)
    }

    private static func jsonObject(from pointer: UnsafeMutablePointer<CChar>) -> [String: Any] {
        let json = String(cString: pointer)
        guard
            let data = json.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return [:]
        }
        return object
    }
}
