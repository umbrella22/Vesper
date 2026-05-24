import Foundation
import VesperPlayerKitBridgeShim

protocol DownloadBindings: Sendable {
    func createDownloadSession(configuration: VesperDownloadConfiguration) -> UInt64

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

    func drainDownloadCommands(
        sessionHandle: UInt64,
        outCommands: inout VesperRuntimeDownloadCommandList
    ) -> Bool

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

    static func remove(_ taskId: UInt64) -> Self {
        Self(kind: .remove, task: nil, taskId: taskId)
    }
}

struct NativeDownloadBindings: DownloadBindings {
    func createDownloadSession(configuration: VesperDownloadConfiguration) -> UInt64 {
        var runtimeConfig = configuration.toRuntimeBridgePayload()
        var handle: UInt64 = 0
        let created = withUnsafePointer(to: &runtimeConfig) { configPointer in
            withUnsafeMutablePointer(to: &handle) { handlePointer in
                vesper_runtime_download_session_create(configPointer, handlePointer)
            }
        }
        freeRuntimeDownloadConfig(&runtimeConfig)
        return created ? handle : 0
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

    func drainDownloadCommands(
        sessionHandle: UInt64,
        outCommands: inout VesperRuntimeDownloadCommandList
    ) -> Bool {
        vesper_runtime_download_session_drain_commands(sessionHandle, &outCommands)
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

func duplicateDownloadCString(_ value: String) -> UnsafeMutablePointer<CChar>? {
    strdup(value)
}

func duplicateDownloadCStringArray(_ values: [String]) -> UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>? {
    guard !values.isEmpty else {
        return nil
    }
    let pointer = UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>.allocate(capacity: values.count)
    for (index, value) in values.enumerated() {
        pointer[index] = duplicateDownloadCString(value)
    }
    return pointer
}

func freeDownloadCStringArray(
    _ values: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) {
    guard let values, count > 0 else {
        return
    }
    for index in 0..<count {
        freeDownloadCString(values[index])
    }
    values.deallocate()
}

func stringFromRuntimeCString(_ pointer: UnsafeMutablePointer<CChar>?) -> String? {
    guard let pointer else {
        return nil
    }
    return String(cString: pointer)
}

func stringArrayFromRuntimeCStringArray(
    _ pointer: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) -> [String] {
    guard let pointer, count > 0 else {
        return []
    }
    return (0..<count).compactMap { index in
        stringFromRuntimeCString(pointer[index])
    }
}

func stringDictionaryFromRuntimeCStringArrays(
    keys: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    values: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    count: Int
) -> [String: String] {
    guard let keys, let values, count > 0 else {
        return [:]
    }
    var result: [String: String] = [:]
    for index in 0..<count {
        guard let key = stringFromRuntimeCString(keys[index]),
              let value = stringFromRuntimeCString(values[index])
        else {
            continue
        }
        result[key] = value
    }
    return result
}

func freeDownloadCString(_ pointer: UnsafeMutablePointer<CChar>?) {
    guard let pointer else {
        return
    }
    free(pointer)
}

func freeRuntimeDownloadSource(_ source: inout VesperRuntimeDownloadSource) {
    freeDownloadCString(source.source_uri)
    freeDownloadCString(source.manifest_uri)
    if let headerNames = source.header_names, source.headers_len > 0 {
        for index in 0..<Int(source.headers_len) {
            freeDownloadCString(headerNames[index])
        }
        headerNames.deallocate()
    }
    if let headerValues = source.header_values, source.headers_len > 0 {
        for index in 0..<Int(source.headers_len) {
            freeDownloadCString(headerValues[index])
        }
        headerValues.deallocate()
    }
    source = VesperRuntimeDownloadSource(
        source_uri: nil,
        content_format: VesperRuntimeDownloadContentFormatUnknown,
        manifest_uri: nil,
        header_names: nil,
        header_values: nil,
        headers_len: 0
    )
}

func freeRuntimeDownloadConfig(_ config: inout VesperRuntimeDownloadConfig) {
    if let pointers = config.plugin_library_paths, config.plugin_library_paths_len > 0 {
        for index in 0..<Int(config.plugin_library_paths_len) {
            freeDownloadCString(pointers[index])
        }
        pointers.deallocate()
    }
    config = VesperRuntimeDownloadConfig(
        auto_start: false,
        run_post_processors_on_completion: false,
        plugin_library_paths: nil,
        plugin_library_paths_len: 0
    )
}

func freeRuntimeDownloadProfile(_ profile: inout VesperRuntimeDownloadProfile) {
    freeDownloadCString(profile.variant_id)
    freeDownloadCString(profile.preferred_audio_language)
    freeDownloadCString(profile.preferred_subtitle_language)
    if let pointers = profile.selected_track_ids, profile.selected_track_ids_len > 0 {
        for index in 0..<Int(profile.selected_track_ids_len) {
            freeDownloadCString(pointers[index])
        }
        pointers.deallocate()
    }
    freeDownloadCString(profile.target_directory)
    profile = VesperRuntimeDownloadProfile(
        variant_id: nil,
        preferred_audio_language: nil,
        preferred_subtitle_language: nil,
        selected_track_ids: nil,
        selected_track_ids_len: 0,
        has_target_output_format: false,
        target_output_format: VesperRuntimeDownloadOutputFormatOriginal,
        target_directory: nil,
        allow_metered_network: false
    )
}

func freeRuntimeDownloadAssetIndex(_ assetIndex: inout VesperRuntimeDownloadAssetIndex) {
    freeDownloadCString(assetIndex.version)
    freeDownloadCString(assetIndex.etag)
    freeDownloadCString(assetIndex.checksum)
    if let resources = assetIndex.resources, assetIndex.resources_len > 0 {
        for index in 0..<Int(assetIndex.resources_len) {
            freeDownloadCString(resources[index].resource_id)
            freeDownloadCString(resources[index].uri)
            freeDownloadCString(resources[index].relative_path)
            freeDownloadCString(resources[index].generated_text)
            freeDownloadCString(resources[index].etag)
            freeDownloadCString(resources[index].checksum)
        }
        resources.deallocate()
    }
    if let segments = assetIndex.segments, assetIndex.segments_len > 0 {
        for index in 0..<Int(assetIndex.segments_len) {
            freeDownloadCString(segments[index].segment_id)
            freeDownloadCString(segments[index].uri)
            freeDownloadCString(segments[index].relative_path)
            freeDownloadCString(segments[index].checksum)
        }
        segments.deallocate()
    }
    if let streams = assetIndex.streams, assetIndex.streams_len > 0 {
        for index in 0..<Int(assetIndex.streams_len) {
            freeDownloadCString(streams[index].stream_id)
            freeDownloadCString(streams[index].language)
            freeDownloadCString(streams[index].codec)
            freeDownloadCString(streams[index].label)
            freeDownloadCStringArray(streams[index].resource_ids, count: Int(streams[index].resource_ids_len))
            freeDownloadCStringArray(streams[index].segment_ids, count: Int(streams[index].segment_ids_len))
            freeDownloadCStringArray(streams[index].metadata_keys, count: Int(streams[index].metadata_len))
            freeDownloadCStringArray(streams[index].metadata_values, count: Int(streams[index].metadata_len))
        }
        streams.deallocate()
    }
    freeDownloadCString(assetIndex.completed_path)
    assetIndex = VesperRuntimeDownloadAssetIndex(
        content_format: VesperRuntimeDownloadContentFormatUnknown,
        version: nil,
        etag: nil,
        checksum: nil,
        has_total_size_bytes: false,
        total_size_bytes: 0,
        resources: nil,
        resources_len: 0,
        segments: nil,
        segments_len: 0,
        streams: nil,
        streams_len: 0,
        completed_path: nil
    )
}

func freeRuntimeDownloadTask(_ task: inout VesperRuntimeDownloadTask) {
    freeDownloadCString(task.asset_id)
    freeRuntimeDownloadSource(&task.source)
    freeRuntimeDownloadProfile(&task.profile)
    freeRuntimeDownloadAssetIndex(&task.asset_index)
    freeDownloadCString(task.error_message)
    task = VesperRuntimeDownloadTask(
        task_id: 0,
        asset_id: nil,
        source: VesperRuntimeDownloadSource(
            source_uri: nil,
            content_format: VesperRuntimeDownloadContentFormatUnknown,
            manifest_uri: nil,
            header_names: nil,
            header_values: nil,
            headers_len: 0
        ),
        profile: VesperRuntimeDownloadProfile(
            variant_id: nil,
            preferred_audio_language: nil,
            preferred_subtitle_language: nil,
            selected_track_ids: nil,
            selected_track_ids_len: 0,
            has_target_output_format: false,
            target_output_format: VesperRuntimeDownloadOutputFormatOriginal,
            target_directory: nil,
            allow_metered_network: false
        ),
        status: VesperRuntimeDownloadTaskStatusQueued,
        progress: VesperRuntimeDownloadProgressSnapshot(
            received_bytes: 0,
            has_total_bytes: false,
            total_bytes: 0,
            received_segments: 0,
            has_total_segments: false,
            total_segments: 0
        ),
        asset_index: VesperRuntimeDownloadAssetIndex(
            content_format: VesperRuntimeDownloadContentFormatUnknown,
            version: nil,
            etag: nil,
            checksum: nil,
            has_total_size_bytes: false,
            total_size_bytes: 0,
            resources: nil,
            resources_len: 0,
            segments: nil,
            segments_len: 0,
            streams: nil,
            streams_len: 0,
            completed_path: nil
        ),
        has_error: false,
        error_code: PlayerFfiErrorCodeNone,
        error_category: PlayerFfiErrorCategoryPlatform,
        error_retriable: false,
        error_message: nil
    )
}

extension VesperDownloadConfiguration {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadConfig {
        let pointer: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
        if pluginLibraryPaths.isEmpty {
            pointer = nil
        } else {
            pointer = .allocate(capacity: pluginLibraryPaths.count)
            for (index, value) in pluginLibraryPaths.enumerated() {
                pointer?[index] = duplicateDownloadCString(value)
            }
        }

        return VesperRuntimeDownloadConfig(
            auto_start: autoStart,
            run_post_processors_on_completion: runPostProcessorsOnCompletion,
            plugin_library_paths: pointer,
            plugin_library_paths_len: UInt(pluginLibraryPaths.count)
        )
    }
}

extension VesperDownloadProgressSnapshot {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadProgressSnapshot {
        VesperRuntimeDownloadProgressSnapshot(
            received_bytes: receivedBytes,
            has_total_bytes: totalBytes != nil,
            total_bytes: totalBytes ?? 0,
            received_segments: receivedSegments,
            has_total_segments: totalSegments != nil,
            total_segments: totalSegments ?? 0
        )
    }
}

extension VesperDownloadTaskSnapshot {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadTask {
        VesperRuntimeDownloadTask(
            task_id: taskId,
            asset_id: duplicateDownloadCString(assetId),
            source: source.toRuntimeBridgePayload(),
            profile: profile.toRuntimeBridgePayload(),
            status: VesperRuntimeDownloadTaskStatus(rawValue: UInt32(state.rawValue)),
            progress: progress.toRuntimeBridgePayload(),
            asset_index: assetIndex.toRuntimeBridgePayload(),
            has_error: error != nil,
            error_code: error?.code.ffiCode ?? PlayerFfiErrorCodeNone,
            error_category: error?.category.ffiCategory ?? PlayerFfiErrorCategoryPlatform,
            error_retriable: error?.retriable ?? false,
            error_message: error.flatMap { duplicateDownloadCString($0.message) }
        )
    }
}

extension VesperDownloadSource {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadSource {
        let headers = sanitizedDownloadHttpHeaders(source.headers)
        let headerNames = Array(headers.keys)
        let headerValues = headerNames.map { headers[$0] ?? "" }
        return VesperRuntimeDownloadSource(
            source_uri: duplicateDownloadCString(source.uri),
            content_format: VesperRuntimeDownloadContentFormat(rawValue: contentFormat.rawValue)
                ?? VesperRuntimeDownloadContentFormatUnknown,
            manifest_uri: manifestUri.flatMap(duplicateDownloadCString),
            header_names: duplicateDownloadCStringArray(headerNames),
            header_values: duplicateDownloadCStringArray(headerValues),
            headers_len: UInt(headerNames.count)
        )
    }
}

extension VesperDownloadProfile {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadProfile {
        let pointer: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
        if selectedTrackIds.isEmpty {
            pointer = nil
        } else {
            pointer = .allocate(capacity: selectedTrackIds.count)
            for (index, value) in selectedTrackIds.enumerated() {
                pointer?[index] = duplicateDownloadCString(value)
            }
        }

        return VesperRuntimeDownloadProfile(
            variant_id: variantId.flatMap(duplicateDownloadCString),
            preferred_audio_language: preferredAudioLanguage.flatMap(duplicateDownloadCString),
            preferred_subtitle_language: preferredSubtitleLanguage.flatMap(duplicateDownloadCString),
            selected_track_ids: pointer,
            selected_track_ids_len: UInt(selectedTrackIds.count),
            has_target_output_format: targetOutputFormat != nil,
            target_output_format: VesperRuntimeDownloadOutputFormat(
                rawValue: UInt32(targetOutputFormat?.rawValue ?? 2)
            ),
            target_directory: targetDirectory.flatMap { duplicateDownloadCString($0.path) },
            allow_metered_network: allowMeteredNetwork
        )
    }
}

extension VesperDownloadResourceRecord {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadResourceRecord {
        VesperRuntimeDownloadResourceRecord(
            resource_id: duplicateDownloadCString(resourceId),
            uri: duplicateDownloadCString(uri),
            relative_path: relativePath.flatMap(duplicateDownloadCString),
            has_byte_range: byteRange != nil,
            byte_range: byteRange?.toRuntimeBridgePayload() ?? VesperRuntimeDownloadByteRange(offset: 0, length: 0),
            generated_text: generatedText.flatMap(duplicateDownloadCString),
            has_size_bytes: sizeBytes != nil,
            size_bytes: sizeBytes ?? 0,
            etag: etag.flatMap(duplicateDownloadCString),
            checksum: checksum.flatMap(duplicateDownloadCString)
        )
    }
}

extension VesperDownloadByteRange {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadByteRange {
        VesperRuntimeDownloadByteRange(offset: offset, length: length)
    }
}

extension VesperDownloadSegmentRecord {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadSegmentRecord {
        VesperRuntimeDownloadSegmentRecord(
            segment_id: duplicateDownloadCString(segmentId),
            uri: duplicateDownloadCString(uri),
            relative_path: relativePath.flatMap(duplicateDownloadCString),
            has_sequence: sequence != nil,
            sequence: sequence ?? 0,
            has_byte_range: byteRange != nil,
            byte_range: byteRange?.toRuntimeBridgePayload() ?? VesperRuntimeDownloadByteRange(offset: 0, length: 0),
            has_size_bytes: sizeBytes != nil,
            size_bytes: sizeBytes ?? 0,
            checksum: checksum.flatMap(duplicateDownloadCString)
        )
    }
}

extension VesperDownloadAssetStream {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadAssetStream {
        let metadataPairs = metadata.sorted { lhs, rhs in lhs.key < rhs.key }
        return VesperRuntimeDownloadAssetStream(
            stream_id: duplicateDownloadCString(streamId),
            kind: kind.toRuntimeBridgePayload(),
            language: language.flatMap(duplicateDownloadCString),
            codec: codec.flatMap(duplicateDownloadCString),
            label: label.flatMap(duplicateDownloadCString),
            has_quality_rank: qualityRank != nil,
            quality_rank: qualityRank ?? 0,
            resource_ids: duplicateDownloadCStringArray(resourceIds),
            resource_ids_len: UInt(resourceIds.count),
            segment_ids: duplicateDownloadCStringArray(segmentIds),
            segment_ids_len: UInt(segmentIds.count),
            metadata_keys: duplicateDownloadCStringArray(metadataPairs.map(\.key)),
            metadata_values: duplicateDownloadCStringArray(metadataPairs.map(\.value)),
            metadata_len: UInt(metadataPairs.count)
        )
    }
}

extension VesperDownloadStreamKind {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadStreamKind {
        switch self {
        case .combined:
            return VesperRuntimeDownloadStreamKindCombined
        case .video:
            return VesperRuntimeDownloadStreamKindVideo
        case .audio:
            return VesperRuntimeDownloadStreamKindAudio
        case .secondaryAudio:
            return VesperRuntimeDownloadStreamKindSecondaryAudio
        case .subtitle:
            return VesperRuntimeDownloadStreamKindSubtitle
        case .auxiliary:
            return VesperRuntimeDownloadStreamKindAuxiliary
        }
    }
}

extension VesperDownloadAssetIndex {
    func toRuntimeBridgePayload() -> VesperRuntimeDownloadAssetIndex {
        let resourcePointer: UnsafeMutablePointer<VesperRuntimeDownloadResourceRecord>?
        if resources.isEmpty {
            resourcePointer = nil
        } else {
            resourcePointer = .allocate(capacity: resources.count)
            for (index, item) in resources.enumerated() {
                resourcePointer?[index] = item.toRuntimeBridgePayload()
            }
        }

        let segmentPointer: UnsafeMutablePointer<VesperRuntimeDownloadSegmentRecord>?
        if segments.isEmpty {
            segmentPointer = nil
        } else {
            segmentPointer = .allocate(capacity: segments.count)
            for (index, item) in segments.enumerated() {
                segmentPointer?[index] = item.toRuntimeBridgePayload()
            }
        }

        let streamPointer: UnsafeMutablePointer<VesperRuntimeDownloadAssetStream>?
        if streams.isEmpty {
            streamPointer = nil
        } else {
            streamPointer = .allocate(capacity: streams.count)
            for (index, item) in streams.enumerated() {
                streamPointer?[index] = item.toRuntimeBridgePayload()
            }
        }

        return VesperRuntimeDownloadAssetIndex(
            content_format: VesperRuntimeDownloadContentFormat(rawValue: contentFormat.rawValue)
                ?? VesperRuntimeDownloadContentFormatUnknown,
            version: version.flatMap(duplicateDownloadCString),
            etag: etag.flatMap(duplicateDownloadCString),
            checksum: checksum.flatMap(duplicateDownloadCString),
            has_total_size_bytes: totalSizeBytes != nil,
            total_size_bytes: totalSizeBytes ?? 0,
            resources: resourcePointer,
            resources_len: UInt(resources.count),
            segments: segmentPointer,
            segments_len: UInt(segments.count),
            streams: streamPointer,
            streams_len: UInt(streams.count),
            completed_path: completedPath.flatMap(duplicateDownloadCString)
        )
    }
}

extension VesperRuntimeDownloadSnapshot {
    func toPublic() -> VesperDownloadSnapshot {
        guard let tasks, len > 0 else {
            return VesperDownloadSnapshot(tasks: [])
        }
        return VesperDownloadSnapshot(
            tasks: Array(UnsafeBufferPointer(start: tasks, count: Int(len))).map { $0.toPublic() }
        )
    }
}

extension VesperRuntimeDownloadTask {
    func toPublic() -> VesperDownloadTaskSnapshot {
        let assetId = stringFromRuntimeCString(asset_id) ?? ""
        let error: VesperDownloadError?
        if has_error {
            error = VesperDownloadError(
                code: VesperPlayerErrorCode(ffiCode: error_code),
                category: VesperPlayerErrorCategory(ffiCategory: error_category),
                retriable: error_retriable,
                message: stringFromRuntimeCString(error_message) ?? "download failed"
            )
        } else {
            error = nil
        }

        return VesperDownloadTaskSnapshot(
            taskId: task_id,
            assetId: assetId,
            source: source.toPublic(),
            profile: profile.toPublic(),
            state: VesperDownloadState(rawValue: Int(status.rawValue)) ?? .queued,
            progress: progress.toPublic(),
            assetIndex: asset_index.toPublic(),
            error: error
        )
    }
}

extension VesperRuntimeDownloadSource {
    func toPublic() -> VesperDownloadSource {
        let uri = stringFromRuntimeCString(source_uri) ?? ""
        let headers = downloadSourceHeaders()
        let source: VesperPlayerSource
        if let url = URL(string: uri), url.isFileURL {
            source = VesperPlayerSource(
                uri: url.absoluteString,
                label: url.lastPathComponent,
                kind: .local,
                protocol: .file,
                headers: headers
            )
        } else if let url = URL(string: uri) {
            source = .remoteUrl(url, headers: headers)
        } else {
            source = VesperPlayerSource(uri: uri, label: uri, kind: .remote, protocol: .unknown, headers: headers)
        }
        return VesperDownloadSource(
            source: source,
            contentFormat: VesperDownloadContentFormat(rawValue: Int(content_format.rawValue)) ?? .unknown,
            manifestUri: stringFromRuntimeCString(manifest_uri)
        )
    }

    private func downloadSourceHeaders() -> [String: String] {
        guard let header_names, let header_values, headers_len > 0 else {
            return [:]
        }
        var headers: [String: String] = [:]
        for index in 0..<Int(headers_len) {
            guard let name = stringFromRuntimeCString(header_names[index]),
                  let value = stringFromRuntimeCString(header_values[index])
            else {
                continue
            }
            headers[name] = value
        }
        return sanitizedDownloadHttpHeaders(headers)
    }
}

extension VesperRuntimeDownloadProfile {
    func toPublic() -> VesperDownloadProfile {
        let selectedTrackIds: [String]
        if let selected_track_ids, selected_track_ids_len > 0 {
            selectedTrackIds = (0..<Int(selected_track_ids_len)).compactMap { index in
                stringFromRuntimeCString(selected_track_ids[index])
            }
        } else {
            selectedTrackIds = []
        }

        return VesperDownloadProfile(
            variantId: stringFromRuntimeCString(variant_id),
            preferredAudioLanguage: stringFromRuntimeCString(preferred_audio_language),
            preferredSubtitleLanguage: stringFromRuntimeCString(preferred_subtitle_language),
            selectedTrackIds: selectedTrackIds,
            targetOutputFormat: has_target_output_format
                ? VesperDownloadOutputFormat(rawValue: Int(target_output_format.rawValue))
                : nil,
            targetDirectory: stringFromRuntimeCString(target_directory).map(URL.init(fileURLWithPath:)),
            allowMeteredNetwork: allow_metered_network
        )
    }
}

extension VesperRuntimeDownloadAssetIndex {
    func toPublic() -> VesperDownloadAssetIndex {
        let publicResources: [VesperDownloadResourceRecord]
        if let resourcesPointer = self.resources, self.resources_len > 0 {
            publicResources = Array(
                UnsafeBufferPointer(start: resourcesPointer, count: Int(self.resources_len))
            )
                .map { $0.toPublic() }
        } else {
            publicResources = []
        }

        let publicSegments: [VesperDownloadSegmentRecord]
        if let segmentsPointer = self.segments, self.segments_len > 0 {
            publicSegments = Array(
                UnsafeBufferPointer(start: segmentsPointer, count: Int(self.segments_len))
            )
                .map { $0.toPublic() }
        } else {
            publicSegments = []
        }

        let publicStreams: [VesperDownloadAssetStream]
        if let streamsPointer = self.streams, self.streams_len > 0 {
            publicStreams = Array(
                UnsafeBufferPointer(start: streamsPointer, count: Int(self.streams_len))
            )
                .map { $0.toPublic() }
        } else {
            publicStreams = []
        }

        return VesperDownloadAssetIndex(
            contentFormat: VesperDownloadContentFormat(rawValue: Int(content_format.rawValue)) ?? .unknown,
            version: stringFromRuntimeCString(version),
            etag: stringFromRuntimeCString(etag),
            checksum: stringFromRuntimeCString(checksum),
            totalSizeBytes: has_total_size_bytes ? total_size_bytes : nil,
            resources: publicResources,
            segments: publicSegments,
            streams: publicStreams,
            completedPath: stringFromRuntimeCString(completed_path)
        )
    }
}

extension VesperRuntimeDownloadResourceRecord {
    func toPublic() -> VesperDownloadResourceRecord {
        VesperDownloadResourceRecord(
            resourceId: stringFromRuntimeCString(resource_id) ?? "",
            uri: stringFromRuntimeCString(uri) ?? "",
            relativePath: stringFromRuntimeCString(relative_path),
            byteRange: has_byte_range ? byte_range.toPublic() : nil,
            generatedText: nil,
            sizeBytes: has_size_bytes ? size_bytes : nil,
            etag: stringFromRuntimeCString(etag),
            checksum: stringFromRuntimeCString(checksum)
        )
    }
}

extension VesperRuntimeDownloadSegmentRecord {
    func toPublic() -> VesperDownloadSegmentRecord {
        VesperDownloadSegmentRecord(
            segmentId: stringFromRuntimeCString(segment_id) ?? "",
            uri: stringFromRuntimeCString(uri) ?? "",
            relativePath: stringFromRuntimeCString(relative_path),
            sequence: has_sequence ? sequence : nil,
            byteRange: has_byte_range ? byte_range.toPublic() : nil,
            sizeBytes: has_size_bytes ? size_bytes : nil,
            checksum: stringFromRuntimeCString(checksum)
        )
    }
}

extension VesperRuntimeDownloadAssetStream {
    func toPublic() -> VesperDownloadAssetStream {
        VesperDownloadAssetStream(
            streamId: stringFromRuntimeCString(stream_id) ?? "",
            kind: kind.toPublic(),
            language: stringFromRuntimeCString(language),
            codec: stringFromRuntimeCString(codec),
            label: stringFromRuntimeCString(label),
            qualityRank: has_quality_rank ? quality_rank : nil,
            resourceIds: stringArrayFromRuntimeCStringArray(resource_ids, count: Int(resource_ids_len)),
            segmentIds: stringArrayFromRuntimeCStringArray(segment_ids, count: Int(segment_ids_len)),
            metadata: stringDictionaryFromRuntimeCStringArrays(
                keys: metadata_keys,
                values: metadata_values,
                count: Int(metadata_len)
            )
        )
    }
}

extension VesperRuntimeDownloadStreamKind {
    func toPublic() -> VesperDownloadStreamKind {
        switch self {
        case VesperRuntimeDownloadStreamKindVideo:
            return .video
        case VesperRuntimeDownloadStreamKindAudio:
            return .audio
        case VesperRuntimeDownloadStreamKindSecondaryAudio:
            return .secondaryAudio
        case VesperRuntimeDownloadStreamKindSubtitle:
            return .subtitle
        case VesperRuntimeDownloadStreamKindAuxiliary:
            return .auxiliary
        default:
            return .combined
        }
    }
}

extension VesperRuntimeDownloadByteRange {
    func toPublic() -> VesperDownloadByteRange {
        VesperDownloadByteRange(offset: offset, length: length)
    }
}

extension VesperRuntimeDownloadProgressSnapshot {
    func toPublic() -> VesperDownloadProgressSnapshot {
        VesperDownloadProgressSnapshot(
            receivedBytes: received_bytes,
            totalBytes: has_total_bytes ? total_bytes : nil,
            receivedSegments: received_segments,
            totalSegments: has_total_segments ? total_segments : nil
        )
    }
}

extension VesperRuntimeDownloadCommandList {
    func toPublic() -> [RuntimeDownloadCommand] {
        guard let commands, len > 0 else {
            return []
        }
        return Array(UnsafeBufferPointer(start: commands, count: Int(len))).compactMap { command in
            switch command.kind {
            case .prepare:
                return .prepare(command.task.toPublic())
            case .start:
                return .start(command.task.toPublic())
            case .pause:
                return .pause(command.task_id)
            case .resume:
                return .resume(command.task.toPublic())
            case .remove:
                return .remove(command.task_id)
            default:
                return nil
            }
        }
    }
}

extension VesperRuntimeDownloadEventList {
    func toPublic() -> [VesperDownloadEvent] {
        guard let events, len > 0 else {
            return []
        }
        let buffer = UnsafeBufferPointer<VesperRuntimeDownloadEvent>(start: events, count: Int(len))
        return buffer.compactMap { event in
            switch event.kind {
            case .created:
                guard let task = event.task else {
                    return nil
                }
                return .created(task.pointee.toPublic())
            case .stateChanged:
                return .stateChanged(
                    VesperDownloadTaskStatePatch(
                        taskId: event.task_id,
                        state: VesperDownloadState(rawValue: Int(event.state_status.rawValue)) ?? .queued,
                        progress: event.state_progress.toPublic(),
                        error: event.state_has_error ? event.toDownloadError() : nil,
                        completedPath: stringFromRuntimeCString(event.state_completed_path)
                    )
                )
            case .assetIndexUpdated:
                guard let task = event.task else {
                    return nil
                }
                return .assetIndexUpdated(task.pointee.toPublic())
            case .progressUpdated:
                return .progressUpdated(
                    VesperDownloadTaskProgressPatch(
                        taskId: event.task_id,
                        progress: event.progress.toPublic()
                    )
                )
            default:
                return nil
            }
        }
    }
}

extension VesperRuntimeDownloadEvent {
    func toDownloadError() -> VesperDownloadError {
        VesperDownloadError(
            code: VesperPlayerErrorCode(ffiCode: state_error_code),
            category: VesperPlayerErrorCategory(ffiCategory: state_error_category),
            retriable: state_error_retriable,
            message: stringFromRuntimeCString(state_error_message) ?? ""
        )
    }
}

extension VesperRuntimeDownloadCommandKind {
    static var prepare: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindPrepare }
    static var start: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindStart }
    static var pause: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindPause }
    static var resume: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindResume }
    static var remove: VesperRuntimeDownloadCommandKind { VesperRuntimeDownloadCommandKindRemove }
}

extension VesperRuntimeDownloadEventKind {
    static var created: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindCreated }
    static var stateChanged: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindStateChanged }
    static var assetIndexUpdated: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindAssetIndexUpdated }
    static var progressUpdated: VesperRuntimeDownloadEventKind { VesperRuntimeDownloadEventKindProgressUpdated }
}

extension VesperRuntimeDownloadContentFormat {
    init?(rawValue: Int) {
        switch rawValue {
        case 0: self = VesperRuntimeDownloadContentFormatHlsSegments
        case 1: self = VesperRuntimeDownloadContentFormatDashSegments
        case 2: self = VesperRuntimeDownloadContentFormatFlvSegments
        case 3: self = VesperRuntimeDownloadContentFormatSingleFile
        case 4: self = VesperRuntimeDownloadContentFormatUnknown
        default: return nil
        }
    }
}

final class DownloadExportProgressBridge: @unchecked Sendable {
    let onProgress: (Float) -> Void
    let isCancelled: () -> Bool

    init(
        onProgress: @escaping (Float) -> Void,
        isCancelled: @escaping () -> Bool
    ) {
        self.onProgress = onProgress
        self.isCancelled = isCancelled
    }

    func retainContext() -> UnsafeMutableRawPointer {
        UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque())
    }

    static func releaseContext(_ context: UnsafeMutableRawPointer?) {
        guard let context else {
            return
        }
        Unmanaged<DownloadExportProgressBridge>.fromOpaque(context).release()
    }

    static func fromContext(_ context: UnsafeMutableRawPointer?) -> DownloadExportProgressBridge? {
        guard let context else {
            return nil
        }
        return Unmanaged<DownloadExportProgressBridge>.fromOpaque(context).takeUnretainedValue()
    }
}

struct DownloadExportBridgeError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}
