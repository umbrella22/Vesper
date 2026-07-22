import Combine
import Foundation
internal import VesperPlayerKitBridgeShim

@MainActor
extension VesperPlaylistCoordinator {
    func refreshSnapshot() {
        let activeItem: VesperPlaylistActiveItem?
        if sessionHandle != 0 {
            var runtimeActiveItem = VesperRuntimePlaylistActiveItem(item_id: nil, index: 0)
            let hasActive = withUnsafeMutablePointer(to: &runtimeActiveItem) { pointer in
                vesper_runtime_playlist_session_current_active_item(sessionHandle, pointer)
            }
            if hasActive, let itemIdPointer = runtimeActiveItem.item_id {
                activeItem = VesperPlaylistActiveItem(
                    itemId: String(cString: itemIdPointer),
                    index: Int(runtimeActiveItem.index)
                )
            } else {
                activeItem = nil
            }
            vesper_runtime_playlist_active_item_free(&runtimeActiveItem)
        } else {
            activeItem = nil
        }

        let hintByItemId = Dictionary(
            uniqueKeysWithValues: viewportHints.map { ($0.itemId, $0.kind) }
        )
        let activeItemId = activeItem?.itemId

        snapshot = VesperPlaylistSnapshot(
            playlistId: configuration.playlistId,
            queue: queue.enumerated().map { index, item in
                VesperPlaylistQueueItemState(
                    item: item,
                    index: index,
                    viewportHint: hintByItemId[item.itemId] ?? .hidden,
                    isActive: activeItemId == item.itemId
                )
            },
            activeItem: activeItem,
            neighborWindow: configuration.neighborWindow,
            preloadWindow: configuration.preloadWindow,
            switchPolicy: configuration.switchPolicy
        )
    }

    func drainAndApplyPreloadCommands() {
        guard sessionHandle != 0 else {
            return
        }
        var commands = VesperRuntimePreloadCommandList(commands: nil, len: 0)
        guard vesper_runtime_playlist_session_drain_preload_commands(sessionHandle, &commands) else {
            return
        }

        let runtimeCommands: [PlaylistWarmupCommand]
        if let pointer = commands.commands, commands.len > 0 {
            runtimeCommands = Array(UnsafeBufferPointer(start: pointer, count: Int(commands.len)))
                .compactMap(PlaylistWarmupCommand.init)
        } else {
            runtimeCommands = []
        }
        vesper_runtime_preload_command_list_free(&commands)

        for command in runtimeCommands {
            switch command {
            case let .start(task):
                startWarmup(task)
            case let .cancel(taskId):
                cancelWarmup(taskId: taskId)
            }
        }
    }
}
