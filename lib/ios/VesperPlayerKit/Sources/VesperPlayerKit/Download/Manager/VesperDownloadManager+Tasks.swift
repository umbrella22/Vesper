import Foundation
import VesperPlayerKitBridgeShim

extension VesperDownloadManager {
    public func createTask(
        assetId: VesperDownloadAssetId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile = VesperDownloadProfile(),
        assetIndex: VesperDownloadAssetIndex = VesperDownloadAssetIndex()
    ) -> VesperDownloadTaskId? {
        guard sessionHandle != 0 else {
            return nil
        }
        let normalizedAssetIndex: VesperDownloadAssetIndex
        do {
            normalizedAssetIndex = try VesperGeneratedDownloadResourceMaterializer(
                baseDirectory: configuration.baseDirectory
            ).materialize(
                assetId: assetId,
                taskId: nil,
                profile: profile,
                assetIndex: assetIndex
            )
        } catch {
            iosHostLog("download generated resource materialization failed: \(error.localizedDescription)")
            return nil
        }

        var runtimeSource = source.toRuntimeBridgePayload()
        var runtimeProfile = profile.toRuntimeBridgePayload()
        var runtimeAssetIndex = normalizedAssetIndex.toRuntimeBridgePayload()
        var taskId: UInt64 = 0
        let created = withUnsafePointer(to: &runtimeSource) { sourcePointer in
            withUnsafePointer(to: &runtimeProfile) { profilePointer in
                withUnsafePointer(to: &runtimeAssetIndex) { assetIndexPointer in
                    withUnsafeMutablePointer(to: &taskId) { taskIdPointer in
                        bindings.createDownloadTask(
                            sessionHandle: sessionHandle,
                            assetId: assetId,
                            source: sourcePointer,
                            profile: profilePointer,
                            assetIndex: assetIndexPointer,
                            outTaskId: taskIdPointer
                        )
                    }
                }
            }
        }
        freeRuntimeDownloadSource(&runtimeSource)
        freeRuntimeDownloadProfile(&runtimeProfile)
        freeRuntimeDownloadAssetIndex(&runtimeAssetIndex)

        guard created, taskId != 0 else {
            return nil
        }
        syncRuntimeState(processCommands: true)
        return taskId
    }

    public func restoreTasks(_ tasks: [VesperDownloadTaskSnapshot]) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        guard !tasks.isEmpty else {
            return true
        }

        let materializer = VesperGeneratedDownloadResourceMaterializer(baseDirectory: configuration.baseDirectory)
        let normalizedTasks: [VesperDownloadTaskSnapshot]
        do {
            normalizedTasks = try tasks.map { task in
                try task.withAssetIndex(
                    materializer.materialize(
                        assetId: task.assetId,
                        taskId: task.taskId,
                        profile: task.profile,
                        assetIndex: task.assetIndex
                    )
                )
            }
        } catch {
            iosHostLog("download state restore failed while materializing generated resources: \(error.localizedDescription)")
            return false
        }

        let pointer = UnsafeMutablePointer<VesperRuntimeDownloadTask>.allocate(capacity: normalizedTasks.count)
        for (index, task) in normalizedTasks.enumerated() {
            pointer[index] = task.toRuntimeBridgePayload()
        }
        let restored = bindings.restoreDownloadTasks(
            sessionHandle: sessionHandle,
            tasks: UnsafePointer(pointer),
            taskCount: normalizedTasks.count
        )
        for index in 0..<normalizedTasks.count {
            freeRuntimeDownloadTask(&pointer[index])
        }
        pointer.deallocate()

        if restored {
            forceFullSync(processCommands: true)
        }
        return restored
    }

    public func startTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        let started = bindings.startDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if started {
            syncRuntimeState(processCommands: true)
        }
        return started
    }

    public func pauseTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        let paused = bindings.pauseDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if paused {
            syncRuntimeState(processCommands: true)
        }
        return paused
    }

    public func resumeTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        let resumed = bindings.resumeDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if resumed {
            syncRuntimeState(processCommands: true)
        }
        return resumed
    }

    public func removeTask(_ taskId: VesperDownloadTaskId) -> Bool {
        guard sessionHandle != 0 else {
            return false
        }
        let removed = bindings.removeDownloadTask(sessionHandle: sessionHandle, taskId: taskId)
        if removed {
            syncRuntimeState(processCommands: true)
        }
        return removed
    }
}
