import Combine
import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

@MainActor
extension VesperPlaylistCoordinator {
    public func replaceQueue(_ queue: [VesperPlaylistQueueItem]) {
        self.queue = queue
        viewportHints = viewportHints.filter { hint in
            queue.contains(where: { $0.itemId == hint.itemId })
        }
        guard sessionHandle != 0 else {
            refreshSnapshot()
            return
        }

        var runtimeQueue = queue.map { $0.toRuntimeBridgePayload() }
        let replaced = runtimeQueue.withUnsafeMutableBufferPointer { buffer in
            vesper_runtime_playlist_session_replace_queue(
                sessionHandle,
                buffer.baseAddress,
                UInt(buffer.count)
            )
        }
        freeRuntimeQueueItems(&runtimeQueue)
        guard replaced else { return }

        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func updateViewportHints(_ hints: [VesperPlaylistViewportHint]) {
        viewportHints = hints
            .filter { $0.kind != .hidden }
            .filter { hint in queue.contains(where: { $0.itemId == hint.itemId }) }
        guard sessionHandle != 0 else {
            refreshSnapshot()
            return
        }

        var runtimeHints = viewportHints.map { $0.toRuntimeBridgePayload() }
        let updated = runtimeHints.withUnsafeMutableBufferPointer { buffer in
            vesper_runtime_playlist_session_update_viewport_hints(
                sessionHandle,
                buffer.baseAddress,
                UInt(buffer.count)
            )
        }
        freeRuntimeViewportHints(&runtimeHints)
        guard updated else { return }

        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func clearViewportHints() {
        viewportHints.removeAll()
        guard sessionHandle != 0 else {
            refreshSnapshot()
            return
        }
        guard vesper_runtime_playlist_session_clear_viewport_hints(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func advanceToNext() {
        guard sessionHandle != 0 else {
            return
        }
        guard vesper_runtime_playlist_session_advance_to_next(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func advanceToPrevious() {
        guard sessionHandle != 0 else {
            return
        }
        guard vesper_runtime_playlist_session_advance_to_previous(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func handlePlaybackCompleted() {
        guard sessionHandle != 0 else {
            return
        }
        guard vesper_runtime_playlist_session_handle_playback_completed(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }

    public func handlePlaybackFailed() {
        guard sessionHandle != 0 else {
            return
        }
        guard vesper_runtime_playlist_session_handle_playback_failed(sessionHandle) else {
            return
        }
        refreshSnapshot()
        drainAndApplyPreloadCommands()
    }
}
