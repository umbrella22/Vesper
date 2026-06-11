@preconcurrency import AVFoundation
import CoreAudio
import Foundation
import VesperPlayerKitBridgeShim

actor VesperNativeFramePipelineRuntime {
    enum CommandResult {
        case success([String: Any])
        case failure(VesperNativeFramePipelineOperationError)
        case ignored
    }

    private weak var owner: VesperNativeFramePipelineSession?
    private let backend: VesperNativeFramePipelineBackend
    private var handle: UInt64 = 0
    private var displayTask: Task<Void, Never>?
    private var isClosed = false
    private var isPlaying = false
    private var playbackRate: Float = 1.0
    private var playbackAnchorMediaUs: Int64?
    private var playbackAnchorHostNs: UInt64?
    private var frameLeaseGeneration: UInt64 = 1

    init(
        owner: VesperNativeFramePipelineSession,
        backend: VesperNativeFramePipelineBackend,
        openedHandle: UInt64 = 0
    ) {
        self.owner = owner
        self.backend = backend
        handle = openedHandle
    }

    func bind(openedHandle: UInt64) {
        handle = openedHandle
    }

    func play(rate: Float) {
        guard handle != 0, !isClosed else { return }
        playbackRate = max(rate, 0.01)
        isPlaying = true
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
        if displayTask == nil {
            displayTask = Task { [weak self] in
                await self?.displayLoop()
            }
        }
    }

    func pause() {
        isPlaying = false
    }

    func setPlaybackRate(_ rate: Float) {
        playbackRate = max(rate, 0.01)
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
    }

    func flush() -> CommandResult {
        guard handle != 0, !isClosed else { return .ignored }
        isPlaying = false
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
        invalidateFrameLeases()
        switch backend.flush(handle: handle) {
        case .success(let object):
            return .success(object)
        case .failure(let error):
            return .failure(error)
        }
    }

    func seek(positionMs: Int64) -> CommandResult {
        guard handle != 0, !isClosed else { return .ignored }
        isPlaying = false
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
        invalidateFrameLeases()
        switch backend.seek(handle: handle, positionMs: positionMs) {
        case .success(let object):
            return .success(object)
        case .failure(let error):
            return .failure(error)
        }
    }

    func close() {
        guard !isClosed else { return }
        isClosed = true
        isPlaying = false
        invalidateFrameLeases()
        displayTask?.cancel()
        displayTask = nil
        if handle != 0 {
            backend.close(handle: handle)
            handle = 0
        }
    }

    private func displayLoop() async {
        while !Task.isCancelled {
            guard !isClosed else { return }
            guard isPlaying else {
                try? await Task.sleep(nanoseconds: 20_000_000)
                continue
            }
            let frame: VesperNativeFramePipelineFrame
            switch advanceFrame() {
            case .frame(let advanced):
                frame = advanced
            case .pending:
                try? await Task.sleep(nanoseconds: 5_000_000)
                continue
            case .endOfStream:
                await owner?.runtimeDidReachEndOfStream()
                pauseForEndOfStream()
                continue
            }
            await waitForPresentationTime(frame.presentationTimeUs)
            guard frameLeaseIsCurrent(frame) else {
                release(frame: frame, presented: false)
                continue
            }
            guard isPlaying else {
                release(frame: frame, presented: false)
                continue
            }
            let presented = await owner?.runtimePresent(frame: frame) ?? false
            guard frameLeaseIsCurrent(frame) else {
                release(frame: frame, presented: false)
                continue
            }
            release(frame: frame, presented: presented)
            if presented, isPlaying {
                let timeline = await owner?.runtimeTimeline(
                    framePresentationTimeUs: frame.presentationTimeUs
                )
                if let timeline {
                    await owner?.runtimeDidPresentFrame(timeline)
                }
            }
        }
    }

    private func pauseForEndOfStream() {
        isPlaying = false
        playbackAnchorMediaUs = nil
        playbackAnchorHostNs = nil
    }

    private func waitForPresentationTime(_ presentationTimeUs: Int64) async {
        let hostNow = DispatchTime.now().uptimeNanoseconds
        if playbackAnchorMediaUs == nil || playbackAnchorHostNs == nil {
            playbackAnchorMediaUs = presentationTimeUs
            playbackAnchorHostNs = hostNow
            return
        }
        guard let anchorMediaUs = playbackAnchorMediaUs,
              let anchorHostNs = playbackAnchorHostNs else {
            return
        }
        let mediaDeltaUs = max(presentationTimeUs - anchorMediaUs, 0)
        let adjustedMediaDeltaUs = UInt64(Double(mediaDeltaUs) / Double(max(playbackRate, 0.01)))
        let mediaDeltaNs = adjustedMediaDeltaUs * 1_000
        let target = anchorHostNs.addingReportingOverflow(mediaDeltaNs)
        guard !target.overflow else {
            return
        }
        let targetHostNs = target.partialValue
        let now = DispatchTime.now().uptimeNanoseconds
        guard targetHostNs > now else {
            return
        }
        try? await Task.sleep(nanoseconds: targetHostNs - now)
    }

    private func advanceFrame() -> VesperNativeFramePipelineAdvanceOutcome {
        guard handle != 0 else { return .pending }
        let object: [String: Any]
        switch backend.advance(handle: handle) {
        case .success(let value):
            object = value
        case .failure(let error):
            iosHostLog("native-frame advance failed: \(error.message)")
            isPlaying = false
            return .pending
        }
        Task { @MainActor [weak owner] in
            owner?.runtimeMergeStatus(object)
        }
        let status = object["status"] as? String
        if status == "endOfStream" {
            return .endOfStream
        }
        guard
            status == "frame",
            let frameHandle = (object["handle"] as? NSNumber)?.uint64Value,
            let pixelBuffer = (object["pixelBuffer"] as? NSNumber)?.uintValue
        else {
            return .pending
        }
        return .frame(
            VesperNativeFramePipelineFrame(
                frameHandle: frameHandle,
                pixelBufferAddress: pixelBuffer,
                presentationTimeUs: (object["presentationTimeUs"] as? NSNumber)?.int64Value ?? 0,
                durationUs: (object["durationUs"] as? NSNumber)?.int64Value,
                width: (object["width"] as? NSNumber)?.intValue ?? 0,
                height: (object["height"] as? NSNumber)?.intValue ?? 0,
                leaseGeneration: frameLeaseGeneration
            )
        )
    }

    private func release(frame: VesperNativeFramePipelineFrame, presented: Bool) {
        guard handle != 0, !isClosed else { return }
        let shouldReportPresented = presented && frameLeaseIsCurrent(frame)
        switch backend.releaseFrame(
            handle: handle,
            frameHandle: frame.frameHandle,
            presented: shouldReportPresented
        ) {
        case .success(let object):
            Task { @MainActor [weak owner] in
                owner?.runtimeMergeStatus(object)
            }
        case .failure(let error):
            iosHostLog("native-frame release failed: \(error.message)")
        }
    }

    private func invalidateFrameLeases() {
        frameLeaseGeneration = frameLeaseGeneration &+ 1
        if frameLeaseGeneration == 0 {
            frameLeaseGeneration = 1
        }
    }

    private func frameLeaseIsCurrent(_ frame: VesperNativeFramePipelineFrame) -> Bool {
        !isClosed && handle != 0 && frame.leaseGeneration == frameLeaseGeneration
    }
}

@MainActor
final class VesperNativeFramePipelineCommandQueue {
    struct Token: Sendable {
        let generation: UInt64
        let sequence: UInt64
    }

    private var tail: Task<Void, Never>?
    private var generation: UInt64 = 1
    private var nextSequence: UInt64 = 1
    private var latestSequence: UInt64 = 0

    @discardableResult
    func submit(_ operation: @escaping @Sendable (Token) async -> Void) -> Token {
        let token = Token(generation: generation, sequence: nextSequence)
        latestSequence = token.sequence
        nextSequence &+= 1
        if nextSequence == 0 {
            nextSequence = 1
        }
        let previous = tail
        tail = Task {
            await previous?.value
            guard !Task.isCancelled else { return }
            guard self.isCurrentGeneration(token) else { return }
            await operation(token)
        }
        return token
    }

    func cancel() {
        generation &+= 1
        if generation == 0 {
            generation = 1
        }
        nextSequence = 1
        latestSequence = 0
        tail?.cancel()
        tail = nil
    }

    func isLatest(_ token: Token) -> Bool {
        token.generation == generation && token.sequence == latestSequence
    }

    private func isCurrentGeneration(_ token: Token) -> Bool {
        token.generation == generation
    }
}
