import Flutter
import Foundation
import VesperPlayerKit

final class DownloadRecoveryContinuationBox: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<VesperDownloadRecoveredTaskPlan?, Error>?
    private var outcome: Result<VesperDownloadRecoveredTaskPlan?, Error>?

    func activate(_ continuation: CheckedContinuation<VesperDownloadRecoveredTaskPlan?, Error>) {
        lock.lock()
        let completed = outcome
        if completed == nil { self.continuation = continuation }
        lock.unlock()
        if let completed { continuation.resume(with: completed) }
    }

    func resume(with result: Result<VesperDownloadRecoveredTaskPlan?, Error>) {
        lock.lock()
        guard outcome == nil else { lock.unlock(); return }
        outcome = result
        let pending = continuation
        continuation = nil
        lock.unlock()
        pending?.resume(with: result)
    }
}

func decodeDownloadRecoveryResponse(_ value: Any?) throws -> VesperDownloadRecoveredTaskPlan? {
    if let failure = value as? FlutterError {
        throw PluginError.operationFailed("Download recovery failed (\(failure.code)): \(failure.message ?? "")")
    }
    if let value = value as AnyObject?, value === FlutterMethodNotImplemented { return nil }
    return try nestedMap(value)?.toDownloadRecoveredTaskPlan()
}

@MainActor
func awaitDownloadRecoveryResponse(
    timeoutNanoseconds: UInt64 = 30_000_000_000,
    invoke: (@escaping FlutterResult) -> Void
) async throws -> VesperDownloadRecoveredTaskPlan? {
    let box = DownloadRecoveryContinuationBox()
    var timeoutTask: Task<Void, Never>?
    defer { timeoutTask?.cancel() }
    return try await withTaskCancellationHandler {
        try Task.checkCancellation()
        return try await withCheckedThrowingContinuation { continuation in
            box.activate(continuation)
            timeoutTask = Task {
                do { try await Task.sleep(nanoseconds: timeoutNanoseconds) }
                catch { return }
                box.resume(with: .success(nil))
            }
            invoke { value in
                box.resume(with: Result { try decodeDownloadRecoveryResponse(value) })
            }
        }
    } onCancel: {
        box.resume(with: .failure(CancellationError()))
    }
}
