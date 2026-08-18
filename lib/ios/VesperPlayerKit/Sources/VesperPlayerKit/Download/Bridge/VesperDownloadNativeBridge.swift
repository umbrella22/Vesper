import Foundation
@_implementationOnly import VesperPlayerKitBridgeShim

protocol DownloadBindings: Sendable {
    func createDownloadSession(configuration: VesperDownloadConfiguration) throws -> UInt64

    func disposeDownloadSession(_ sessionHandle: UInt64)

    func createDownloadTask(
        sessionHandle: UInt64,
        assetId: String,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>,
        outTaskId: UnsafeMutablePointer<UInt64>
    ) -> Bool

    func restoreDownloadTasks(
        sessionHandle: UInt64,
        tasks: UnsafePointer<VesperRuntimeDownloadTask>?,
        taskCount: Int
    ) -> Bool

    func startDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

    func pauseDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

    func resumeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

    func updateDownloadProgress(
        sessionHandle: UInt64,
        taskId: UInt64,
        receivedBytes: UInt64,
        receivedSegments: UInt32
    ) -> Bool

    func completeDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        completedPath: String?
    ) -> Bool

    func completeDownloadPreparation(
        sessionHandle: UInt64,
        taskId: UInt64,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool

    func replaceDownloadTaskPlan(
        sessionHandle: UInt64,
        taskId: UInt64,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool

    func exportDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        outputPath: String,
        onProgress: @escaping (Float) -> Void,
        isCancelled: @escaping () -> Bool
    ) throws

    func failDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        error: VesperDownloadError
    ) -> Bool

    func removeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool

    func downloadSessionSnapshot(
        sessionHandle: UInt64,
        outSnapshot: inout VesperRuntimeDownloadSnapshot
    ) -> Bool

    func peekDownloadCommands(
        sessionHandle: UInt64,
        outCommands: inout VesperRuntimeDownloadCommandList
    ) -> Bool

    func acknowledgeDownloadCommands(sessionHandle: UInt64, commandCount: UInt) -> Bool

    func drainDownloadEvents(
        sessionHandle: UInt64,
        outEvents: inout VesperRuntimeDownloadEventList
    ) -> Bool

    func freeDownloadSnapshot(_ snapshot: inout VesperRuntimeDownloadSnapshot)

    func freeDownloadCommandList(_ commands: inout VesperRuntimeDownloadCommandList)

    func freeDownloadEventList(_ events: inout VesperRuntimeDownloadEventList)
}


struct RuntimeDownloadCommand {
    enum Kind {
        case prepare
        case start
        case pause
        case resume
        case remove
    }

    let kind: Kind
    let task: VesperDownloadTaskSnapshot?
    let taskId: UInt64

    static func prepare(_ task: VesperDownloadTaskSnapshot) -> Self {
        Self(kind: .prepare, task: task, taskId: task.taskId)
    }

    static func start(_ task: VesperDownloadTaskSnapshot) -> Self {
        Self(kind: .start, task: task, taskId: task.taskId)
    }

    static func resume(_ task: VesperDownloadTaskSnapshot) -> Self {
        Self(kind: .resume, task: task, taskId: task.taskId)
    }

    static func pause(_ taskId: UInt64) -> Self {
        Self(kind: .pause, task: nil, taskId: taskId)
    }

    static func remove(_ task: VesperDownloadTaskSnapshot) -> Self {
        Self(kind: .remove, task: task, taskId: task.taskId)
    }
}

struct NativeDownloadBindings: DownloadBindings {
    func createDownloadSession(configuration: VesperDownloadConfiguration) throws -> UInt64 {
        let pluginRegistry: VesperEmbeddedPluginRegistry
        let payload: VesperRuntimeDownloadConfig
        do {
            pluginRegistry = try VesperEmbeddedPluginRegistry.create(
                references: configuration.postDownloadPluginReferences
                    + configuration.eventHookPluginReferences
            )
            payload = try configuration.toRuntimeBridgePayload(
                pluginRegistryHandle: pluginRegistry.handle
            )
        } catch {
            throw VesperDownloadManagerInitializationError.pluginConfiguration(
                error.localizedDescription
            )
        }
        var runtimeConfig = payload
        defer { freeRuntimeDownloadConfig(&runtimeConfig) }
        var handle: UInt64 = 0
        var errorMessage: UnsafeMutablePointer<CChar>?
        let created = withExtendedLifetime(pluginRegistry) {
            withUnsafePointer(to: &runtimeConfig) { configPointer in
                withUnsafeMutablePointer(to: &handle) { handlePointer in
                    withUnsafeMutablePointer(to: &errorMessage) { errorPointer in
                        vesper_runtime_download_session_create(
                            configPointer,
                            handlePointer,
                            errorPointer
                        )
                    }
                }
            }
        }
        defer {
            if let errorMessage {
                vesper_runtime_download_error_string_free(errorMessage)
            }
        }
        guard created, handle != 0 else {
            throw VesperDownloadManagerInitializationError.nativeSessionCreation(
                errorMessage.map { String(cString: $0) }
                    ?? "Native download session creation failed."
            )
        }
        return handle
    }

    func disposeDownloadSession(_ sessionHandle: UInt64) {
        vesper_runtime_download_session_dispose(sessionHandle)
    }

    func createDownloadTask(
        sessionHandle: UInt64,
        assetId: String,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>,
        outTaskId: UnsafeMutablePointer<UInt64>
    ) -> Bool {
        assetId.withCString { assetIdPointer in
            vesper_runtime_download_session_create_task(
                sessionHandle,
                assetIdPointer,
                source,
                profile,
                assetIndex,
                outTaskId
            )
        }
    }

    func restoreDownloadTasks(
        sessionHandle: UInt64,
        tasks: UnsafePointer<VesperRuntimeDownloadTask>?,
        taskCount: Int
    ) -> Bool {
        vesper_runtime_download_session_restore_tasks(
            sessionHandle,
            tasks,
            taskCount
        )
    }

    func startDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_start_task(sessionHandle, taskId)
    }

    func pauseDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_pause_task(sessionHandle, taskId)
    }

    func resumeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_resume_task(sessionHandle, taskId)
    }

    func updateDownloadProgress(
        sessionHandle: UInt64,
        taskId: UInt64,
        receivedBytes: UInt64,
        receivedSegments: UInt32
    ) -> Bool {
        vesper_runtime_download_session_update_progress(
            sessionHandle,
            taskId,
            receivedBytes,
            receivedSegments
        )
    }

    func completeDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        completedPath: String?
    ) -> Bool {
        guard let completedPath else {
            return vesper_runtime_download_session_complete_task(sessionHandle, taskId, nil)
        }
        return completedPath.withCString { pathPointer in
            vesper_runtime_download_session_complete_task(
                sessionHandle,
                taskId,
                pathPointer
            )
        }
    }

    func completeDownloadPreparation(
        sessionHandle: UInt64,
        taskId: UInt64,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool {
        vesper_runtime_download_session_complete_preparation(
            sessionHandle,
            taskId,
            assetIndex
        )
    }

    func replaceDownloadTaskPlan(
        sessionHandle: UInt64,
        taskId: UInt64,
        source: UnsafePointer<VesperRuntimeDownloadSource>,
        profile: UnsafePointer<VesperRuntimeDownloadProfile>,
        assetIndex: UnsafePointer<VesperRuntimeDownloadAssetIndex>
    ) -> Bool {
        vesper_runtime_download_session_replace_task_plan(
            sessionHandle,
            taskId,
            source,
            profile,
            assetIndex
        )
    }

    func exportDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        outputPath: String,
        onProgress: @escaping (Float) -> Void,
        isCancelled: @escaping () -> Bool
    ) throws {
        let bridge = DownloadExportProgressBridge(
            onProgress: onProgress,
            isCancelled: isCancelled
        )
        let context = bridge.retainContext()
        let callbacks = VesperRuntimeDownloadExportCallbacks(
            context: context,
            on_progress: { context, ratio in
                DownloadExportProgressBridge.fromContext(context)?.onProgress(ratio)
            },
            is_cancelled: { context in
                DownloadExportProgressBridge.fromContext(context)?.isCancelled() ?? false
            }
        )
        let exported = outputPath.withCString { outputPathPointer in
            vesper_runtime_download_session_export_task(
                sessionHandle,
                taskId,
                outputPathPointer,
                callbacks
            )
        }
        DownloadExportProgressBridge.releaseContext(context)
        if !exported {
            throw DownloadExportBridgeError("download export failed for task \(taskId)")
        }
    }

    func failDownloadTask(
        sessionHandle: UInt64,
        taskId: UInt64,
        error: VesperDownloadError
    ) -> Bool {
        error.message.withCString { messagePointer in
            vesper_runtime_download_session_fail_task(
                sessionHandle,
                taskId,
                error.code.ffiCode,
                error.category.ffiCategory,
                error.retriable,
                messagePointer
            )
        }
    }

    func removeDownloadTask(sessionHandle: UInt64, taskId: UInt64) -> Bool {
        vesper_runtime_download_session_remove_task(sessionHandle, taskId)
    }

    func downloadSessionSnapshot(
        sessionHandle: UInt64,
        outSnapshot: inout VesperRuntimeDownloadSnapshot
    ) -> Bool {
        vesper_runtime_download_session_snapshot(sessionHandle, &outSnapshot)
    }

    func peekDownloadCommands(
        sessionHandle: UInt64,
        outCommands: inout VesperRuntimeDownloadCommandList
    ) -> Bool {
        vesper_runtime_download_session_peek_commands(sessionHandle, &outCommands)
    }

    func acknowledgeDownloadCommands(sessionHandle: UInt64, commandCount: UInt) -> Bool {
        vesper_runtime_download_session_acknowledge_commands(sessionHandle, commandCount)
    }

    func drainDownloadEvents(
        sessionHandle: UInt64,
        outEvents: inout VesperRuntimeDownloadEventList
    ) -> Bool {
        vesper_runtime_download_session_drain_events(sessionHandle, &outEvents)
    }

    func freeDownloadSnapshot(_ snapshot: inout VesperRuntimeDownloadSnapshot) {
        vesper_runtime_download_snapshot_free(&snapshot)
    }

    func freeDownloadCommandList(_ commands: inout VesperRuntimeDownloadCommandList) {
        vesper_runtime_download_command_list_free(&commands)
    }

    func freeDownloadEventList(_ events: inout VesperRuntimeDownloadEventList) {
        vesper_runtime_download_event_list_free(&events)
    }
}
