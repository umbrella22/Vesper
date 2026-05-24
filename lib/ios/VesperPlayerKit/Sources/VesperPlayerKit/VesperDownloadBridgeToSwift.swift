import Foundation
import VesperPlayerKitBridgeShim

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

