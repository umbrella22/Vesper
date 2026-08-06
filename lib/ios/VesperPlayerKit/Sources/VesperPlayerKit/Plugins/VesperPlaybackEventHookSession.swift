import Foundation
internal import VesperPlayerKitBridgeShim

/// Owns one Rust playback EventHook dispatcher for an AVPlayer session.
///
/// The dispatcher has a bounded queue. Event submission is intentionally
/// best-effort; dropped work and plugin outcomes are read from `drainReports`.
final class VesperPlaybackEventHookSession {
    private var handle: UInt64
    private var isClosed = false

    init(configuration: VesperPipelineEventHookConfiguration) throws {
        let pluginRegistry = try VesperEmbeddedPluginRegistry.create(
            references: configuration.pluginReferences
        )
        let referencesJSON = try encodeVesperPluginReferencesJSON(configuration.pluginReferences)
        var handle: UInt64 = 0
        var errorMessage: UnsafeMutablePointer<CChar>?
        let created = withExtendedLifetime(pluginRegistry) {
            referencesJSON.withCString { referencesPointer in
                withUnsafeMutablePointer(to: &handle) { handlePointer in
                    withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
                        vesper_runtime_playback_event_hook_session_create(
                            pluginRegistry.handle,
                            referencesPointer,
                            handlePointer,
                            errorPointer
                        )
                    }
                }
            }
        }
        defer { freePlaybackEventHookCString(errorMessage) }
        guard created, handle != 0 else {
            throw VesperPlaybackEventHookSessionError.bridgeError(
                stringFromPlaybackEventHookCString(errorMessage)
                    ?? "playback EventHook session create failed"
            )
        }
        self.handle = handle
    }

    deinit {
        dispose()
    }

    @discardableResult
    func submit(
        runId: String,
        sessionId: String,
        protocolName: String?,
        eventName: String,
        timestampNs: UInt64,
        thread: String? = "main",
        resourceIdentity: String?,
        attributes: [String: String] = [:]
    ) -> Bool {
        guard handle != 0, !isClosed else { return false }
        var event: [String: Any] = [
            "runId": runId,
            "sessionId": sessionId,
            "platform": "ios",
            "eventName": eventName,
            "timestampNs": timestampNs,
            "attributes": attributes,
            "diagnostic": NSNull(),
        ]
        event["protocol"] = protocolName ?? NSNull()
        event["thread"] = thread ?? NSNull()
        event["resourceIdentity"] = resourceIdentity ?? NSNull()
        guard JSONSerialization.isValidJSONObject(event),
              let data = try? JSONSerialization.data(withJSONObject: event, options: [.sortedKeys]),
              let json = String(data: data, encoding: .utf8)
        else {
            return false
        }
        return json.withCString { pointer in
            var errorMessage: UnsafeMutablePointer<CChar>?
            let submitted = withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
                vesper_runtime_playback_event_hook_session_submit_json(
                    handle,
                    pointer,
                    errorPointer
                )
            }
            freePlaybackEventHookCString(errorMessage)
            return submitted
        }
    }

    @discardableResult
    func flush(timeoutMs: UInt64 = 2_000) -> Bool {
        guard handle != 0 else { return true }
        var errorMessage: UnsafeMutablePointer<CChar>?
        let flushed = withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
            vesper_runtime_playback_event_hook_session_flush(
                handle,
                timeoutMs,
                errorPointer
            )
        }
        freePlaybackEventHookCString(errorMessage)
        return flushed
    }

    func drainReports() -> VesperPipelineEventHookReportBatch {
        guard handle != 0 else { return VesperPipelineEventHookReportBatch() }
        var reportPointer: UnsafeMutablePointer<CChar>?
        var errorMessage: UnsafeMutablePointer<CChar>?
        let drained = withUnsafeMutablePointer(to: &reportPointer) { reportOutput in
            withUnsafeMutablePointer(to: &errorMessage) { errorOutput in
                vesper_runtime_playback_event_hook_session_drain_json(
                    handle,
                    reportOutput,
                    errorOutput
                )
            }
        }
        defer {
            freePlaybackEventHookCString(reportPointer)
            freePlaybackEventHookCString(errorMessage)
        }
        guard drained, let reportPointer else {
            return decodeVesperPipelineEventHookReportBatch(
                data: nil,
                bridgeError: stringFromPlaybackEventHookCString(errorMessage)
            )
        }
        return decodeVesperPipelineEventHookReportBatch(
            data: String(cString: reportPointer).data(using: .utf8),
            bridgeError: stringFromPlaybackEventHookCString(errorMessage)
        )
    }

    @discardableResult
    func close() -> Bool {
        guard handle != 0, !isClosed else { return true }
        var errorMessage: UnsafeMutablePointer<CChar>?
        let closed = withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
            vesper_runtime_playback_event_hook_session_close(handle, errorPointer)
        }
        freePlaybackEventHookCString(errorMessage)
        if closed {
            isClosed = true
        }
        return closed
    }

    func dispose() {
        guard handle != 0 else { return }
        if !isClosed {
            _ = close()
        }
        vesper_runtime_playback_event_hook_session_dispose(handle)
        handle = 0
    }
}

private enum VesperPlaybackEventHookSessionError: LocalizedError {
    case bridgeError(String)

    var errorDescription: String? {
        switch self {
        case let .bridgeError(message): message
        }
    }
}

private func freePlaybackEventHookCString(_ pointer: UnsafeMutablePointer<CChar>?) {
    guard let pointer else { return }
    vesper_runtime_playback_event_hook_report_string_free(pointer)
}

private func stringFromPlaybackEventHookCString(
    _ pointer: UnsafeMutablePointer<CChar>?
) -> String? {
    guard let pointer else { return nil }
    return String(cString: pointer)
}
