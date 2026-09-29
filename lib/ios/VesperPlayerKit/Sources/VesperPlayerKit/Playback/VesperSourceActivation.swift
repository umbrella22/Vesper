import Foundation

public struct VesperSourceActivationOptions: Equatable, Sendable {
    public let playWhenReady: Bool
    public let startPositionMs: Int64
    public let playbackRate: Float
    public let timeoutMs: UInt64
    public init(playWhenReady: Bool = true, startPositionMs: Int64 = 0,
                playbackRate: Float = 1, timeoutMs: UInt64 = 30_000) {
        self.playWhenReady = playWhenReady; self.startPositionMs = startPositionMs
        self.playbackRate = playbackRate; self.timeoutMs = timeoutMs
    }
    internal func validate() throws {
        guard startPositionMs >= 0, playbackRate.isFinite, playbackRate > 0,
              (1...60_000).contains(timeoutMs) else { throw VesperSourceActivationError.invalidOptions }
    }
}

public struct VesperSourceActivation: Equatable, Sendable {
    public let activationId: String
    public let sessionId: String
    public let sourceId: String
    public let playbackEpoch: UInt64
    public var wire: [String: Any] {
        ["activationId": activationId, "sessionId": sessionId, "sourceId": sourceId, "playbackEpoch": playbackEpoch]
    }
}

public enum VesperSourceActivationError: String, Error, Sendable {
    case superseded, disposed, detached, timeout, cancelled, invalidOptions, unsupported
}

@MainActor
internal final class VesperPendingSourceActivation {
    let id = UUID().uuidString
    let lease: VesperSourceLease
    let continuation: CheckedContinuation<VesperSourceActivation, Error>
    var worker: Task<Void, Never>?
    var timeout: Task<Void, Never>?
    init(lease: VesperSourceLease, continuation: CheckedContinuation<VesperSourceActivation, Error>) {
        self.lease = lease; self.continuation = continuation
    }
}

@MainActor
extension VesperPlayerController {
    public func activate(_ handle: VesperSourceHandle,
                         options: VesperSourceActivationOptions = .init()) async throws -> VesperSourceActivation {
        try ensureStandaloneSourceActivation()
        return try await activateLease(handle.acquire(), options: options)
    }

    internal func activateLease(_ lease: VesperSourceLease,
                                options: VesperSourceActivationOptions) async throws -> VesperSourceActivation {
        try options.validate()
        guard !isDisposed else { throw VesperSourceActivationError.disposed }
        guard handlePlaybackEpochImpl() != nil else { throw VesperSourceActivationError.unsupported }
        let source = try lease.sourceForActivation()

        try Task.checkCancellation()
        // Selection's bridge command owns asynchronous readiness and epoch fences.
        cancelSourceActivation(reason: .superseded)
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let pending = VesperPendingSourceActivation(lease: lease, continuation: continuation)
                pendingHandleActivation = pending
                let load = startHandleActivationImpl(source)
                activeSourceLease?.release()
                activeSourceLease = nil
                pending.worker = Task { [weak self, pending] in
                    do {
                        try await load.value
                        guard let self, self.pendingHandleActivation === pending else { return }
                        _ = try lease.sourceForActivation()
                        if options.startPositionMs > 0 { try await self.seekHandlePositionImpl(options.startPositionMs) }
                        guard self.pendingHandleActivation === pending else { return }
                        _ = try lease.sourceForActivation()
                        guard let playbackEpoch = self.handlePlaybackEpochImpl(), playbackEpoch > 0 else {
                            throw VesperSourceActivationError.unsupported
                        }
                        self.setPlaybackRate(options.playbackRate)
                        if options.playWhenReady { self.play() } else { self.pause() }
                        guard self.pendingHandleActivation === pending else { return }
                        self.pendingHandleActivation = nil
                        pending.timeout?.cancel()
                        self.activeSourceLease = lease
                        pending.continuation.resume(returning: .init(activationId: pending.id, sessionId: lease.sessionId,
                                                                     sourceId: lease.handleId, playbackEpoch: playbackEpoch))
                    } catch {

                        guard let self, self.pendingHandleActivation === pending else { return }
                        self.pendingHandleActivation = nil
                        pending.timeout?.cancel()
                        lease.release()
                        pending.continuation.resume(throwing: error)
                    }
                }
                pending.timeout = Task { [weak self, pending] in
                    do { try await Task.sleep(nanoseconds: options.timeoutMs * 1_000_000) } catch { return }
                    guard let self, self.pendingHandleActivation === pending else { return }
                    self.cancelSourceActivation(reason: .timeout)
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in
                guard let self, self.pendingHandleActivation?.lease === lease else { return }
                self.cancelSourceActivation(reason: .cancelled)
            }
        }
    }

    internal func cancelSourceActivation(reason: VesperSourceActivationError) {
        guard let pending = pendingHandleActivation else { return }
        pendingHandleActivation = nil
        pending.worker?.cancel()
        pending.timeout?.cancel()
        cancelHandleActivationImpl()
        pending.lease.release()
        pending.continuation.resume(throwing: reason)
    }
}
