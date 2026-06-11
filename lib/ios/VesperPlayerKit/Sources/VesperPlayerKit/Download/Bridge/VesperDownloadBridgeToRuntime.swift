import Foundation
import VesperPlayerKitBridgeShim

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

