import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

extension VesperDownloadManager {
    final class RuntimeReporter: VesperDownloadExecutionReporter {
        private weak var manager: VesperDownloadManager?

        init(manager: VesperDownloadManager) {
            self.manager = manager
        }

        func completePreparation(
            taskId: VesperDownloadTaskId,
            assetIndex: VesperDownloadAssetIndex
        ) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            var runtimeAssetIndex = assetIndex.toRuntimeBridgePayload()
            _ = withUnsafePointer(to: &runtimeAssetIndex) { assetIndexPointer in
                manager.bindings.completeDownloadPreparation(
                    sessionHandle: manager.sessionHandle,
                    taskId: taskId,
                    assetIndex: assetIndexPointer
                )
            }
            freeRuntimeDownloadAssetIndex(&runtimeAssetIndex)
            manager.syncRuntimeState(processCommands: true)
        }

        func replaceTaskPlan(
            taskId: VesperDownloadTaskId,
            source: VesperDownloadSource,
            profile: VesperDownloadProfile,
            assetIndex: VesperDownloadAssetIndex
        ) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            var runtimeSource = source.toRuntimeBridgePayload()
            var runtimeProfile = profile.toRuntimeBridgePayload()
            var runtimeAssetIndex = assetIndex.toRuntimeBridgePayload()
            _ = withUnsafePointer(to: &runtimeSource) { sourcePointer in
                withUnsafePointer(to: &runtimeProfile) { profilePointer in
                    withUnsafePointer(to: &runtimeAssetIndex) { assetIndexPointer in
                        manager.bindings.replaceDownloadTaskPlan(
                            sessionHandle: manager.sessionHandle,
                            taskId: taskId,
                            source: sourcePointer,
                            profile: profilePointer,
                            assetIndex: assetIndexPointer
                        )
                    }
                }
            }
            freeRuntimeDownloadSource(&runtimeSource)
            freeRuntimeDownloadProfile(&runtimeProfile)
            freeRuntimeDownloadAssetIndex(&runtimeAssetIndex)
            manager.syncRuntimeState(processCommands: false)
        }

        func updateProgress(
            taskId: VesperDownloadTaskId,
            receivedBytes: UInt64,
            receivedSegments: UInt32
        ) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            _ = manager.bindings.updateDownloadProgress(
                sessionHandle: manager.sessionHandle,
                taskId: taskId,
                receivedBytes: receivedBytes,
                receivedSegments: receivedSegments
            )
            manager.syncRuntimeState(processCommands: false)
        }

        func complete(taskId: VesperDownloadTaskId, completedPath: String?) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            _ = manager.bindings.completeDownloadTask(
                sessionHandle: manager.sessionHandle,
                taskId: taskId,
                completedPath: completedPath
            )
            manager.syncRuntimeState(processCommands: false)
        }

        func fail(taskId: VesperDownloadTaskId, error: VesperDownloadError) {
            guard let manager, manager.sessionHandle != 0 else {
                return
            }
            _ = manager.bindings.failDownloadTask(
                sessionHandle: manager.sessionHandle,
                taskId: taskId,
                error: error
            )
            manager.syncRuntimeState(processCommands: false)
        }
    }
}
