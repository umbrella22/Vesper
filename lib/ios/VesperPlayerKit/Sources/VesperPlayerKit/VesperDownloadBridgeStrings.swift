import Foundation
import VesperPlayerKitBridgeShim

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

