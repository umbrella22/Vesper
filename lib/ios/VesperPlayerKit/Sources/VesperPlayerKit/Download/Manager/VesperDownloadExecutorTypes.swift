import Combine
import Foundation
internal import VesperPlayerKitBridgeShim
#if canImport(UIKit)
import UIKit
#endif
@MainActor
public protocol VesperDownloadExecutionReporter: AnyObject {
    func completePreparation(
        taskId: VesperDownloadTaskId,
        assetIndex: VesperDownloadAssetIndex
    )

    func replaceTaskPlan(
        taskId: VesperDownloadTaskId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    )

    func updateProgress(
        taskId: VesperDownloadTaskId,
        receivedBytes: UInt64,
        receivedSegments: UInt32
    )

    func complete(
        taskId: VesperDownloadTaskId,
        completedPath: String?
    )

    func fail(
        taskId: VesperDownloadTaskId,
        error: VesperDownloadError
    )
}

public protocol VesperDownloadExecutor: AnyObject {
    func prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    )

    func start(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    )

    func resume(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    )

    func pause(taskId: VesperDownloadTaskId)

    func remove(task: VesperDownloadTaskSnapshot?)

    func dispose()
}

public extension VesperDownloadExecutionReporter {
    func replaceTaskPlan(
        taskId: VesperDownloadTaskId,
        source: VesperDownloadSource,
        profile: VesperDownloadProfile,
        assetIndex: VesperDownloadAssetIndex
    ) {}
}

public extension VesperDownloadExecutor {
    func prepare(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        Task { @MainActor in
            reporter.completePreparation(taskId: task.taskId, assetIndex: task.assetIndex)
        }
    }

    func resume(
        task: VesperDownloadTaskSnapshot,
        reporter: any VesperDownloadExecutionReporter
    ) {
        start(task: task, reporter: reporter)
    }

    func pause(taskId: VesperDownloadTaskId) {}

    func remove(task: VesperDownloadTaskSnapshot?) {}

    func dispose() {}
}
