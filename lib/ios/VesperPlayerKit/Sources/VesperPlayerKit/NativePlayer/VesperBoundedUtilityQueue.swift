@preconcurrency import Foundation

final class VesperBoundedUtilityQueue: @unchecked Sendable {
    static let shared = VesperBoundedUtilityQueue()

    private let lock = NSLock()
    private let queue: OperationQueue
    private let maxPendingOperations: Int
    private var pendingOperations = 0
    private let maxRequiredWaiters: Int
    private var requiredWaiters: [CheckedContinuation<Bool, Never>] = []

    init(
        maxConcurrentOperations: Int = 2,
        maxPendingOperations: Int = 16,
        maxRequiredWaiters: Int? = nil
    ) {
        self.maxPendingOperations = max(maxPendingOperations, maxConcurrentOperations)
        self.maxRequiredWaiters = max(maxRequiredWaiters ?? maxPendingOperations, 0)
        queue = OperationQueue()
        queue.name = "io.github.umbrella22.vesper.player.utility"
        queue.qualityOfService = .utility
        queue.maxConcurrentOperationCount = max(maxConcurrentOperations, 1)
    }

    func run<T>(
        fallback: @escaping () -> T,
        _ work: @escaping () -> T
    ) async -> T {
        guard reserveOperation() else {
            return fallback()
        }
        return await withCheckedContinuation { continuation in
            queue.addOperation { [weak self] in
                defer { self?.releaseOperation() }
                continuation.resume(returning: work())
            }
        }
    }

    func runVoid(_ work: @escaping () -> Void) async {
        _ = await run(fallback: { () }) {
            work()
        }
    }

    func runRequiredVoid(_ work: @escaping () -> Void) async {
        guard await reserveRequiredOperation() else {
            // Required cleanup is never dropped. When both the execution slots
            // and waiter budget are full, run inline to apply backpressure
            // instead of escaping to an unbounded dispatch pool.
            work()
            return
        }
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
            queue.addOperation { [weak self] in
                defer {
                    self?.releaseOperation()
                    continuation.resume()
                }
                work()
            }
        }
    }

    private func reserveOperation() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard pendingOperations < maxPendingOperations else {
            return false
        }
        pendingOperations += 1
        return true
    }

    private func reserveRequiredOperation() async -> Bool {
        if reserveOperation() {
            return true
        }
        return await withCheckedContinuation { continuation in
            lock.lock()
            if pendingOperations < maxPendingOperations {
                pendingOperations += 1
                lock.unlock()
                continuation.resume(returning: true)
                return
            }
            guard requiredWaiters.count < maxRequiredWaiters else {
                lock.unlock()
                continuation.resume(returning: false)
                return
            }
            requiredWaiters.append(continuation)
            lock.unlock()
        }
    }

    private func releaseOperation() {
        var waiter: CheckedContinuation<Bool, Never>?
        lock.lock()
        if requiredWaiters.isEmpty {
            pendingOperations = max(0, pendingOperations - 1)
        } else {
            waiter = requiredWaiters.removeFirst()
        }
        lock.unlock()
        waiter?.resume(returning: true)
    }
}
