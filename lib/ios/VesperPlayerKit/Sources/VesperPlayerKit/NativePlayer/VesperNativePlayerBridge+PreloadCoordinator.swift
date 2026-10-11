import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

@MainActor
final class VesperNativePreloadCoordinator {
    struct WarmupRequest: Sendable {
        let sessionHandle: UInt64
        let taskId: UInt64
        let memoryBytes: UInt64
        let windowMs: UInt64
        let url: URL
        let headers: [String: String]
    }

    typealias WarmupExecution = @Sendable (WarmupRequest) async -> Void
    private let budgetPolicy: VesperPreloadBudgetPolicy
    private let execute: WarmupExecution
    private var cachePolicy: ResolvedCachePolicy = .disabled
    private var warmupTask: Task<Void, Never>?
    private(set) var sessionHandle: UInt64 = 0
    private(set) var isClosed = false

    init(
        budgetPolicy: VesperPreloadBudgetPolicy,
        execute: @escaping WarmupExecution = VesperNativePreloadCoordinator.executeWarmup
    ) {
        self.budgetPolicy = budgetPolicy
        self.execute = execute
    }

    func configure(cachePolicy: ResolvedCachePolicy) {
        guard !isClosed else { return }
        self.cachePolicy = cachePolicy
    }

    func warmCurrentSource(source: VesperPlayerSource, url: URL) {
        guard !isClosed else { return }
        cancelCurrent()
        guard source.drmConfiguration == nil, source.dashStartupScope == nil,
              cachePolicy.enabled, max(cachePolicy.memoryCapacity, cachePolicy.diskCapacity) > 0 else { return }

        sessionHandle = createPreloadSession(budgetPolicy)
        guard sessionHandle != 0 else { return }
        let handle = sessionHandle
        let candidate = runtimePreloadCandidate(source: source)
        guard planPreloadCandidates(handle: handle, candidates: [candidate]) else { return }

        for command in drainPreloadCommands(handle: handle) {
            switch command.kind {
            case .start:
                // Only copied scalar fields are used after the native command list is freed.
                // Detached work uses this session identity, never a later replacement.
                let request = WarmupRequest(
                    sessionHandle: handle, taskId: command.task.task_id,
                    memoryBytes: command.task.expected_memory_bytes, windowMs: command.task.warmup_window_ms,
                    url: url, headers: source.headers
                )
                let execute = self.execute
                warmupTask = Task.detached(priority: .utility) { await execute(request) }
            case .cancel:
                warmupTask?.cancel()
            default:
                continue
            }
        }
    }

    // A source switch ends this planner's reservations, but allows the next source to warm.
    func cancelCurrent() {
        warmupTask?.cancel()
        warmupTask = nil
        if sessionHandle != 0 {
            vesper_runtime_preload_session_dispose(sessionHandle)
            sessionHandle = 0
        }
    }

    func close() {
        guard !isClosed else { return }
        isClosed = true
        cancelCurrent()
    }

    deinit {
        warmupTask?.cancel()
        if sessionHandle != 0 { vesper_runtime_preload_session_dispose(sessionHandle) }
    }

    private func runtimePreloadCandidate(source: VesperPlayerSource) -> VesperRuntimePreloadCandidate {
        VesperRuntimePreloadCandidate(
            source_uri: duplicateCString(source.uri),
            scope_kind: VesperRuntimePreloadScopeKindApp,
            scope_id: nil,
            candidate_kind: VesperRuntimePreloadCandidateKindCurrent,
            selection_hint: VesperRuntimePreloadSelectionHintCurrentItem,
            priority: VesperRuntimePreloadPriorityCritical,
            expected_memory_bytes: UInt64(max(budgetPolicy.maxMemoryBytes ?? 32 * 1024, 0)),
            expected_disk_bytes: UInt64(max(budgetPolicy.maxDiskBytes ?? 0, 0)),
            has_ttl_ms: true,
            ttl_ms: UInt64(max(budgetPolicy.warmupWindowMs ?? 30_000, 0)),
            has_warmup_window_ms: true,
            warmup_window_ms: UInt64(max(budgetPolicy.warmupWindowMs ?? 30_000, 0))
        )
    }

    nonisolated private static func executeWarmup(_ request: WarmupRequest) async {
        guard request.memoryBytes > 0 else {
            guard !Task.isCancelled else { return }
            _ = vesper_runtime_preload_session_complete(request.sessionHandle, request.taskId)
            return
        }
        let warmupBytes = Int(clamping: request.memoryBytes)
        var httpRequest = URLRequest(url: request.url)
        applyHttpHeaders(request.headers, to: &httpRequest)
        httpRequest.cachePolicy = .returnCacheDataElseLoad
        httpRequest.timeoutInterval = TimeInterval(max(Int64(clamping: request.windowMs), 1_000)) / 1000.0
        httpRequest.setValue("bytes=0-\(max(warmupBytes - 1, 0))", forHTTPHeaderField: "Range")

        do {
            try Task.checkCancellation()
            let response = try await VesperSequenceURLSessionWarmupLoader().load(request: httpRequest, maximumBytes: warmupBytes)
            try Task.checkCancellation()
            iosHostLog("preload warmup completed status=\(response.statusCode) url=\(diagnosticURLDescription(request.url.absoluteString))")
            _ = vesper_runtime_preload_session_complete(request.sessionHandle, request.taskId)
        } catch {
            guard !Task.isCancelled else { return }
            iosHostLog("preload warmup failed: \(error.localizedDescription)")
            error.localizedDescription.withCString { message in
                _ = vesper_runtime_preload_session_fail(
                    request.sessionHandle, request.taskId,
                    PlayerFfiErrorCodeBackendFailure, PlayerFfiErrorCategoryNetwork, false, message
                )
            }
        }
    }
}

func createPreloadSession(_ budgetPolicy: VesperPreloadBudgetPolicy) -> UInt64 {
    var resolved = VesperRuntimeResolvedPreloadBudgetPolicy(
        max_concurrent_tasks: encodeRuntimeUInt32(
            budgetPolicy.maxConcurrentTasks,
            field: "maxConcurrentTasks"
        ),
        max_memory_bytes: budgetPolicy.maxMemoryBytes ?? 0,
        max_disk_bytes: budgetPolicy.maxDiskBytes ?? 0,
        warmup_window_ms: UInt64(max(budgetPolicy.warmupWindowMs ?? 0, 0))
    )
    var handle: UInt64 = 0
    let created = withUnsafePointer(to: &resolved) { resolvedPointer in
        withUnsafeMutablePointer(to: &handle) { handlePointer in
            vesper_runtime_preload_session_create(resolvedPointer, handlePointer)
        }
    }
    return created ? handle : 0
}

func planPreloadCandidates(
    handle: UInt64,
    candidates: [VesperRuntimePreloadCandidate]
) -> Bool {
    guard !candidates.isEmpty else { return true }
    var mutableCandidates = candidates
    let planned = mutableCandidates.withUnsafeMutableBufferPointer { buffer in
        vesper_runtime_preload_session_plan(handle, buffer.baseAddress, UInt(buffer.count))
    }
    for candidate in mutableCandidates {
        if let sourceUri = candidate.source_uri {
            free(UnsafeMutablePointer(mutating: sourceUri))
        }
    }
    return planned
}

func drainPreloadCommands(handle: UInt64) -> [VesperRuntimePreloadCommand] {
    var commands = VesperRuntimePreloadCommandList(commands: nil, len: 0)
    guard vesper_runtime_preload_session_drain_commands(handle, &commands),
          let commandPointer = commands.commands,
          commands.len > 0
    else {
        return []
    }

    let result = Array(UnsafeBufferPointer(start: commandPointer, count: Int(commands.len)))
    vesper_runtime_preload_command_list_free(&commands)
    return result
}

func duplicateCString(_ value: String) -> UnsafePointer<CChar>? {
    let duplicated = strdup(value)
    guard let duplicated else {
        return nil
    }
    return UnsafePointer(duplicated)
}

private extension VesperRuntimePreloadCommandKind {
    static var start: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindStart
    }

    static var cancel: VesperRuntimePreloadCommandKind {
        VesperRuntimePreloadCommandKindCancel
    }
}
