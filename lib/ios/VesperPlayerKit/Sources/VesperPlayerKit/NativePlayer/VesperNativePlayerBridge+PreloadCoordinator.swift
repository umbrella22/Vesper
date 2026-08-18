import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

final class VesperNativePreloadCoordinator {
    private let budgetPolicy: VesperPreloadBudgetPolicy
    private var cachePolicy: ResolvedCachePolicy = .disabled
    private var warmupTask: Task<Void, Never>?
    private var sessionHandle: UInt64 = 0

    init(budgetPolicy: VesperPreloadBudgetPolicy) {
        self.budgetPolicy = budgetPolicy
        sessionHandle = createPreloadSession(budgetPolicy)
    }

    func configure(cachePolicy: ResolvedCachePolicy) {
        self.cachePolicy = cachePolicy
    }

    func warmCurrentSource(source: VesperPlayerSource, url: URL) {
        cancelWarmupOnly()
        guard source.drmConfiguration == nil else {
            return
        }
        guard max(cachePolicy.memoryCapacity, cachePolicy.diskCapacity) > 0 else {
            return
        }

        let candidate = runtimePreloadCandidate(source: source)
        guard planPreloadCandidates(handle: sessionHandle, candidates: [candidate]) else {
            return
        }

        let commands = drainPreloadCommands(handle: sessionHandle)
        for command in commands {
            switch command.kind {
            case .start:
                let task = command.task
                let headers = source.headers
                warmupTask = Task.detached(priority: .utility) {
                    await Self.executeWarmup(
                        handle: self.sessionHandle,
                        task: task,
                        url: url,
                        headers: headers
                    )
                }
            case .cancel:
                warmupTask?.cancel()
            default:
                continue
            }
        }
    }

    deinit {
        cancelWarmupOnly()
        if sessionHandle != 0 {
            vesper_runtime_preload_session_dispose(sessionHandle)
            sessionHandle = 0
        }
    }

    func cancelAll() {
        cancelWarmupOnly()
        if sessionHandle != 0 {
            vesper_runtime_preload_session_dispose(sessionHandle)
            sessionHandle = 0
        }
    }

    private func cancelWarmupOnly() {
        warmupTask?.cancel()
        warmupTask = nil
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

    private static func executeWarmup(
        handle: UInt64,
        task: VesperRuntimePreloadTask,
        url: URL,
        headers: [String: String]
    ) async {
        let warmupBytes = max(Int64(task.expected_memory_bytes), 1)
        var request = URLRequest(url: url)
        applyHttpHeaders(headers, to: &request)
        request.cachePolicy = .returnCacheDataElseLoad
        request.timeoutInterval = TimeInterval(max(Int64(task.warmup_window_ms), 1_000)) / 1000.0
        request.setValue("bytes=0-\(max(warmupBytes - 1, 0))", forHTTPHeaderField: "Range")

        do {
            let (_, response) = try await URLSession.shared.data(for: request)
            if let httpResponse = response as? HTTPURLResponse {
                iosHostLog(
                    "preload warmup completed status=\(httpResponse.statusCode) url=\(diagnosticURLDescription(url.absoluteString))"
                )
            }
            _ = vesper_runtime_preload_session_complete(handle, task.task_id)
        } catch {
            iosHostLog("preload warmup failed: \(error.localizedDescription)")
            error.localizedDescription.withCString { message in
                _ = vesper_runtime_preload_session_fail(
                    handle,
                    task.task_id,
                    PlayerFfiErrorCodeBackendFailure,
                    PlayerFfiErrorCategoryNetwork,
                    false,
                    message
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
