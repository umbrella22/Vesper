import Foundation

extension VesperForegroundDownloadExecutor {
    func prepareAssetIndexWithRecovery(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) async throws -> VesperDownloadAssetIndex {
        do {
            let assetIndex = try await prepareAssetIndex(task: task)
            return try materializeGeneratedResources(
                assetId: task.assetId,
                taskId: task.taskId,
                profile: task.profile,
                assetIndex: assetIndex
            )
        } catch let error as VesperStaleDownloadResourceError {
            let staleResource = error.staleResource(taskId: task.taskId, phase: .prepare)
            guard let recoveredPlan = await recoverTaskPlan(task: task, staleResource: staleResource) else {
                throw error
            }
            let materializedRecoveredIndex = try materializeGeneratedResources(
                assetId: task.assetId,
                taskId: task.taskId,
                profile: recoveredPlan.profile,
                assetIndex: recoveredPlan.assetIndex
            )
            let recoveredTask = VesperDownloadTaskSnapshot(
                taskId: task.taskId,
                assetId: task.assetId,
                source: recoveredPlan.source,
                profile: recoveredPlan.profile,
                state: task.state,
                progress: task.progress,
                assetIndex: materializedRecoveredIndex,
                error: task.error
            )
            await reporter.replaceTaskPlan(
                taskId: task.taskId,
                source: recoveredPlan.source,
                profile: recoveredPlan.profile,
                assetIndex: materializedRecoveredIndex
            )
            let assetIndex = try await prepareAssetIndex(task: recoveredTask)
            let materializedAssetIndex = try materializeGeneratedResources(
                assetId: task.assetId,
                taskId: task.taskId,
                profile: recoveredPlan.profile,
                assetIndex: assetIndex
            )
            storeRecoveredSource(recoveredPlan.source, forTaskId: task.taskId)
            return materializedAssetIndex
        }
    }

    func recoverTaskPlan(
        task: VesperDownloadTaskSnapshot,
        staleResource: VesperDownloadStaleResource
    ) async -> VesperDownloadRecoveredTaskPlan? {
        if let staleResourcePlanRecoveryHandler {
            return await staleResourcePlanRecoveryHandler(task, staleResource)
        }
        guard let staleResourceRecoveryHandler,
              let source = await staleResourceRecoveryHandler(task, staleResource)
        else {
            return nil
        }
        return VesperDownloadRecoveredTaskPlan(
            source: source,
            profile: task.profile,
            assetIndex: VesperDownloadAssetIndex()
        )
    }

    func materializeGeneratedResources(
        assetId: VesperDownloadAssetId,
        taskId: VesperDownloadTaskId?,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) throws -> VesperDownloadAssetIndex {
        try VesperGeneratedDownloadResourceMaterializer(
            fileManager: fileManager,
            baseDirectory: baseDirectory
        ).materialize(
            assetId: assetId,
            taskId: taskId,
            profile: profile,
            assetIndex: assetIndex
        )
    }

    func storeRecoveredSource(_ source: VesperDownloadSource, forTaskId taskId: VesperDownloadTaskId) {
        lock.lock()
        recoveredSources[taskId] = source
        lock.unlock()
    }

    func taskWithRecoveredSource(_ task: VesperDownloadTaskSnapshot) -> VesperDownloadTaskSnapshot {
        lock.lock()
        let recoveredSource = recoveredSources[task.taskId]
        lock.unlock()
        guard let recoveredSource else {
            return task
        }
        return VesperDownloadTaskSnapshot(
            taskId: task.taskId,
            assetId: task.assetId,
            source: recoveredSource,
            profile: task.profile,
            state: task.state,
            progress: task.progress,
            assetIndex: task.assetIndex,
            error: task.error
        )
    }
}
