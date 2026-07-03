import Combine
import Foundation
import VesperPlayerKitBridgeShim

@MainActor
extension VesperPlaylistCoordinator {
    func startWarmup(_ task: PlaylistWarmupTask) {
        cancelWarmup(taskId: task.taskId)
        guard let source = sourceForWarmup(uri: task.sourceUri) else {
            return
        }

        if source.source.drmConfiguration != nil {
            drmUnsupportedRouteMessage("playlistPreload").withCString { message in
                _ = vesper_runtime_playlist_session_fail_preload_task(
                    sessionHandle,
                    task.taskId,
                    PlayerFfiErrorCodeUnsupported,
                    PlayerFfiErrorCategoryCapability,
                    false,
                    message
                )
            }
            return
        }

        let resolvedResiliencePolicy = resiliencePolicy.resolvedForRuntimeSource(source.source)
        let cachePolicy = playlistResolvedCachePolicy(resolvedResiliencePolicy.cache)
        VesperPlaylistSharedUrlCacheCoordinator.shared.apply(
            policy: cachePolicy,
            token: cachePolicyToken
        )

        let handle = sessionHandle
        let headers = source.source.headers
        warmupTasks[task.taskId] = Task.detached(priority: .utility) {
            guard !Task.isCancelled else { return }
            var request = URLRequest(url: source.url)
            applyHttpHeaders(headers, to: &request)
            request.cachePolicy = .returnCacheDataElseLoad
            let clampedWarmupWindowMs = Int64(min(task.warmupWindowMs, UInt64(Int64.max)))
            request.timeoutInterval = TimeInterval(max(clampedWarmupWindowMs, 1_000)) / 1000.0
            let clampedExpectedMemoryBytes = Int64(min(task.expectedMemoryBytes, UInt64(Int64.max)))
            let warmupBytes = max(clampedExpectedMemoryBytes, 1)
            request.setValue("bytes=0-\(max(warmupBytes - 1, 0))", forHTTPHeaderField: "Range")

            do {
                try Task.checkCancellation()
                _ = try await URLSession.shared.data(for: request)
                try Task.checkCancellation()
                _ = vesper_runtime_playlist_session_complete_preload_task(handle, task.taskId)
            } catch is CancellationError {
            } catch {
                error.localizedDescription.withCString { message in
                    _ = vesper_runtime_playlist_session_fail_preload_task(
                        handle,
                        task.taskId,
                        PlayerFfiErrorCodeBackendFailure,
                        PlayerFfiErrorCategoryNetwork,
                        false,
                        message
                    )
                }
            }

            _ = await MainActor.run {
                if !Task.isCancelled {
                    self.warmupTasks.removeValue(forKey: task.taskId)
                }
            }
        }
    }

    func cancelWarmup(taskId: UInt64) {
        warmupTasks.removeValue(forKey: taskId)?.cancel()
    }

    func cancelAllWarmups() {
        let tasks = warmupTasks.values
        warmupTasks.removeAll()
        tasks.forEach { $0.cancel() }
    }

    func sourceForWarmup(uri: String) -> PlaylistWarmupSource? {
        if let source = queue.first(where: { $0.source.uri == uri })?.source,
           let url = URL(string: source.uri)
        {
            return PlaylistWarmupSource(source: source, url: url)
        }

        guard let url = URL(string: uri) else {
            return nil
        }
        if url.isFileURL {
            return PlaylistWarmupSource(source: .localFile(url: url), url: url)
        }
        return PlaylistWarmupSource(source: .remoteUrl(url), url: url)
    }
}

struct PlaylistWarmupSource {
    let source: VesperPlayerSource
    let url: URL
}

struct PlaylistWarmupTask {
    let taskId: UInt64
    let sourceUri: String
    let expectedMemoryBytes: UInt64
    let warmupWindowMs: UInt64
}

enum PlaylistWarmupCommand {
    case start(PlaylistWarmupTask)
    case cancel(UInt64)

    init?(_ command: VesperRuntimePreloadCommand) {
        switch command.kind {
        case .playlistStart:
            guard let sourceUri = command.task.source_uri else {
                return nil
            }
            self = .start(
                PlaylistWarmupTask(
                    taskId: command.task.task_id,
                    sourceUri: String(cString: sourceUri),
                    expectedMemoryBytes: command.task.expected_memory_bytes,
                    warmupWindowMs: command.task.warmup_window_ms
                )
            )
        case .playlistCancel:
            self = .cancel(command.task_id)
        default:
            return nil
        }
    }
}
