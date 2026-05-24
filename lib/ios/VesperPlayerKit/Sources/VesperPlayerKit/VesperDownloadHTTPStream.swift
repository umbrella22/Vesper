import Foundation

struct VesperHTTPBodyStream {
    let response: URLResponse
    let chunks: AsyncThrowingStream<Data, Error>
    let cancel: @Sendable () -> Void
}

final class VesperURLSessionDataStreamDelegate: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let sourceDescription: String
    private let stalledTransferTimeoutNs: UInt64
    private let lock = NSLock()
    private let watchdogQueue: DispatchQueue
    private let chunksContinuation: AsyncThrowingStream<Data, Error>.Continuation
    private var responseContinuation: CheckedContinuation<URLResponse, Error>?
    private var responseResult: Result<URLResponse, Error>?
    private var session: URLSession?
    private var task: URLSessionDataTask?
    private var watchdog: DispatchSourceTimer?
    private var lastActivityNs: UInt64
    private var didFinish = false

    let chunks: AsyncThrowingStream<Data, Error>

    init(stalledTransferTimeoutMs: UInt64, sourceDescription: String) {
        self.sourceDescription = sourceDescription
        let (timeoutNs, overflow) = stalledTransferTimeoutMs.multipliedReportingOverflow(by: 1_000_000)
        stalledTransferTimeoutNs = overflow ? UInt64.max : timeoutNs
        watchdogQueue = DispatchQueue(
            label: "io.github.ikaros.vesper.player.download.http-watchdog.\(UUID().uuidString)"
        )
        lastActivityNs = DispatchTime.now().uptimeNanoseconds

        var continuation: AsyncThrowingStream<Data, Error>.Continuation!
        chunks = AsyncThrowingStream(Data.self, bufferingPolicy: .unbounded) { streamContinuation in
            continuation = streamContinuation
        }
        chunksContinuation = continuation
        super.init()
        chunksContinuation.onTermination = { @Sendable [weak self] _ in
            self?.cancel()
        }
    }

    func bind(session: URLSession, task: URLSessionDataTask) {
        lock.lock()
        self.session = session
        self.task = task
        lastActivityNs = DispatchTime.now().uptimeNanoseconds
        lock.unlock()
        startWatchdogIfNeeded()
    }

    func waitForResponse() async throws -> URLResponse {
        if let result = lockedResponseResult() {
            return try result.get()
        }
        return try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            if let responseResult {
                lock.unlock()
                continuation.resume(with: responseResult)
            } else {
                responseContinuation = continuation
                lock.unlock()
            }
        }
    }

    func cancel() {
        var localTask: URLSessionDataTask?
        var localSession: URLSession?
        lock.lock()
        localTask = task
        localSession = session
        lock.unlock()
        localTask?.cancel()
        localSession?.invalidateAndCancel()
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping (URLSession.ResponseDisposition) -> Void
    ) {
        markActivity()
        completeResponse(.success(response))
        completionHandler(.allow)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        guard !data.isEmpty else { return }
        markActivity()
        chunksContinuation.yield(data)
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error {
            finish(throwing: error)
        } else {
            finish()
        }
    }

    private func lockedResponseResult() -> Result<URLResponse, Error>? {
        lock.lock()
        defer { lock.unlock() }
        return responseResult
    }

    private func completeResponse(_ result: Result<URLResponse, Error>) {
        var continuation: CheckedContinuation<URLResponse, Error>?
        lock.lock()
        if responseResult == nil {
            responseResult = result
            continuation = responseContinuation
            responseContinuation = nil
        }
        lock.unlock()
        continuation?.resume(with: result)
    }

    private func markActivity() {
        lock.lock()
        lastActivityNs = DispatchTime.now().uptimeNanoseconds
        lock.unlock()
    }

    private func startWatchdogIfNeeded() {
        guard stalledTransferTimeoutNs > 0 else { return }
        let timer = DispatchSource.makeTimerSource(queue: watchdogQueue)
        let interval = DispatchTimeInterval.nanoseconds(
            Int(min(stalledTransferTimeoutNs, UInt64(Int.max)))
        )
        timer.schedule(deadline: .now() + interval, repeating: interval)
        timer.setEventHandler { [weak self] in
            self?.failIfStalled()
        }
        lock.lock()
        watchdog = timer
        lock.unlock()
        timer.resume()
    }

    private func failIfStalled() {
        let shouldFail: Bool
        lock.lock()
        let elapsedNs = DispatchTime.now().uptimeNanoseconds - lastActivityNs
        shouldFail = !didFinish && stalledTransferTimeoutNs > 0 && elapsedNs >= stalledTransferTimeoutNs
        lock.unlock()
        guard shouldFail else { return }
        let error = VesperForegroundDownloadPreparationError.invalidSource(
            "network transfer stalled without progress for \(sourceDescription)"
        )
        finish(throwing: error)
        task?.cancel()
        session?.invalidateAndCancel()
    }

    private func finish(throwing error: Error? = nil) {
        var shouldFinishStream = false
        var localWatchdog: DispatchSourceTimer?
        var localSession: URLSession?
        lock.lock()
        if !didFinish {
            didFinish = true
            shouldFinishStream = true
            localWatchdog = watchdog
            watchdog = nil
            localSession = session
        }
        lock.unlock()

        if let error {
            completeResponse(.failure(error))
        } else {
            completeResponse(.failure(VesperForegroundDownloadPreparationError.invalidSource(
                "remote resource did not return a response for \(sourceDescription)"
            )))
        }
        localWatchdog?.cancel()
        if shouldFinishStream {
            if let error {
                chunksContinuation.finish(throwing: error)
            } else {
                chunksContinuation.finish()
            }
            localSession?.finishTasksAndInvalidate()
        }
    }
}

