import Foundation
internal import VesperPlayerKitBridgeShim

private func decodeRuntimeRecords<T, U>(
    _ pointer: UnsafeMutablePointer<T>?,
    count: UInt,
    decode: (T) -> U?
) -> [U]? {
    guard let count = Int(exactly: count) else {
        return nil
    }
    guard count > 0 else {
        return pointer == nil ? [] : nil
    }
    guard let pointer else {
        return nil
    }
    var decoded: [U] = []
    decoded.reserveCapacity(count)
    for record in UnsafeBufferPointer(start: pointer, count: count) {
        guard let value = decode(record) else {
            return nil
        }
        decoded.append(value)
    }
    return decoded
}

extension VesperRuntimeDownloadSnapshot {
    func decodePublic() -> VesperDownloadSnapshot? {
        guard let decodedTasks = decodeRuntimeRecords(tasks, count: len, decode: { $0.decodePublic() }) else {
            return nil
        }
        return VesperDownloadSnapshot(tasks: decodedTasks)
    }
}

extension VesperRuntimeDownloadTask {
    func decodePublic() -> VesperDownloadTaskSnapshot? {
        guard task_id != 0,
              let assetId = requiredRuntimeCString(asset_id),
              let state = VesperDownloadState(rawValue: Int(status.rawValue)),
              let source = source.decodePublic(),
              let profile = profile.decodePublic(),
              let assetIndex = asset_index.decodePublic()
        else {
            return nil
        }
        let error: VesperDownloadError?
        if has_error {
            guard let message = stringFromRuntimeCString(error_message) else {
                return nil
            }
            error = VesperDownloadError(
                code: VesperPlayerErrorCode(ffiCode: error_code),
                category: VesperPlayerErrorCategory(ffiCategory: error_category),
                retriable: error_retriable,
                message: message
            )
        } else {
            error = nil
        }

        return VesperDownloadTaskSnapshot(
            taskId: task_id,
            assetId: assetId,
            source: source,
            profile: profile,
            state: state,
            progress: progress.toPublic(),
            assetIndex: assetIndex,
            error: error
        )
    }
}

extension VesperRuntimeDownloadSource {
    func decodePublic() -> VesperDownloadSource? {
        guard let uri = requiredRuntimeCString(source_uri),
              let headers = downloadSourceHeaders(),
              let contentFormat = content_format.decodePublic()
        else {
            return nil
        }
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
            contentFormat: contentFormat,
            manifestUri: stringFromRuntimeCString(manifest_uri)
        )
    }

    private func downloadSourceHeaders() -> [String: String]? {
        guard let names = decodeRuntimeCStringArray(header_names, count: headers_len),
              let values = decodeRuntimeCStringArray(header_values, count: headers_len),
              names.count == values.count
        else {
            return nil
        }
        var headers: [String: String] = [:]
        for (name, value) in zip(names, values) {
            guard headers.updateValue(value, forKey: name) == nil else {
                return nil
            }
        }
        return sanitizedDownloadHttpHeaders(headers)
    }
}

extension VesperRuntimeDownloadProfile {
    func decodePublic() -> VesperDownloadProfile? {
        guard let selectedTrackIds = decodeRuntimeCStringArray(
            selected_track_ids,
            count: selected_track_ids_len
        ), selectedTrackIds.allSatisfy({ !$0.isEmpty }) else {
            return nil
        }

        let targetOutputFormat: VesperDownloadOutputFormat?
        if has_target_output_format {
            guard let decoded = VesperDownloadOutputFormat(
                rawValue: Int(target_output_format.rawValue)
            ) else {
                return nil
            }
            targetOutputFormat = decoded
        } else {
            targetOutputFormat = nil
        }

        return VesperDownloadProfile(
            variantId: stringFromRuntimeCString(variant_id),
            preferredAudioLanguage: stringFromRuntimeCString(preferred_audio_language),
            preferredSubtitleLanguage: stringFromRuntimeCString(preferred_subtitle_language),
            selectedTrackIds: selectedTrackIds,
            targetOutputFormat: targetOutputFormat,
            targetDirectory: stringFromRuntimeCString(target_directory).map(URL.init(fileURLWithPath:)),
            allowMeteredNetwork: allow_metered_network
        )
    }
}

extension VesperRuntimeDownloadAssetIndex {
    func decodePublic() -> VesperDownloadAssetIndex? {
        guard let contentFormat = content_format.decodePublic(),
              let publicResources = decodeRuntimeRecords(
                resources,
                count: resources_len,
                decode: { $0.decodePublic() }
              ),
              let publicSegments = decodeRuntimeRecords(
                segments,
                count: segments_len,
                decode: { $0.decodePublic() }
              ),
              let publicStreams = decodeRuntimeRecords(
                streams,
                count: streams_len,
                decode: { $0.decodePublic() }
              )
        else {
            return nil
        }

        return VesperDownloadAssetIndex(
            contentFormat: contentFormat,
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
    func decodePublic() -> VesperDownloadResourceRecord? {
        guard let resourceId = requiredRuntimeCString(resource_id),
              let uri = requiredRuntimeCString(uri)
        else {
            return nil
        }
        return VesperDownloadResourceRecord(
            resourceId: resourceId,
            uri: uri,
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
    func decodePublic() -> VesperDownloadSegmentRecord? {
        guard let segmentId = requiredRuntimeCString(segment_id),
              let uri = requiredRuntimeCString(uri)
        else {
            return nil
        }
        return VesperDownloadSegmentRecord(
            segmentId: segmentId,
            uri: uri,
            relativePath: stringFromRuntimeCString(relative_path),
            sequence: has_sequence ? sequence : nil,
            byteRange: has_byte_range ? byte_range.toPublic() : nil,
            sizeBytes: has_size_bytes ? size_bytes : nil,
            checksum: stringFromRuntimeCString(checksum)
        )
    }
}

extension VesperRuntimeDownloadAssetStream {
    func decodePublic() -> VesperDownloadAssetStream? {
        guard let streamId = requiredRuntimeCString(stream_id),
              let streamKind = kind.decodePublic(),
              let resourceIds = decodeRuntimeCStringArray(resource_ids, count: resource_ids_len),
              let segmentIds = decodeRuntimeCStringArray(segment_ids, count: segment_ids_len),
              let metadata = decodeRuntimeStringDictionary(
                keys: metadata_keys,
                values: metadata_values,
                count: metadata_len
              ),
              resourceIds.allSatisfy({ !$0.isEmpty }),
              segmentIds.allSatisfy({ !$0.isEmpty }),
              metadata.keys.allSatisfy({ !$0.isEmpty })
        else {
            return nil
        }
        return VesperDownloadAssetStream(
            streamId: streamId,
            kind: streamKind,
            language: stringFromRuntimeCString(language),
            codec: stringFromRuntimeCString(codec),
            label: stringFromRuntimeCString(label),
            qualityRank: has_quality_rank ? quality_rank : nil,
            resourceIds: resourceIds,
            segmentIds: segmentIds,
            metadata: metadata
        )
    }
}

extension VesperRuntimeDownloadStreamKind {
    func decodePublic() -> VesperDownloadStreamKind? {
        switch self {
        case VesperRuntimeDownloadStreamKindCombined:
            return .combined
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
            return nil
        }
    }
}

extension VesperRuntimeDownloadContentFormat {
    func decodePublic() -> VesperDownloadContentFormat? {
        VesperDownloadContentFormat(rawValue: Int(rawValue))
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
    func decodePublicCommands() -> [RuntimeDownloadCommand]? {
        decodeRuntimeRecords(commands, count: len) { command in
            switch command.kind {
            case .prepare:
                guard command.task_id != 0,
                      let task = command.task.decodePublic(),
                      task.taskId == command.task_id
                else {
                    return nil
                }
                return .prepare(task)
            case .start:
                guard command.task_id != 0,
                      let task = command.task.decodePublic(),
                      task.taskId == command.task_id
                else {
                    return nil
                }
                return .start(task)
            case .pause:
                guard command.task_id != 0 else {
                    return nil
                }
                return .pause(command.task_id)
            case .resume:
                guard command.task_id != 0,
                      let task = command.task.decodePublic(),
                      task.taskId == command.task_id
                else {
                    return nil
                }
                return .resume(task)
            case .remove:
                guard command.task_id != 0,
                      let task = command.task.decodePublic(),
                      task.taskId == command.task_id
                else {
                    return nil
                }
                return .remove(task)
            default:
                return nil
            }
        }
    }
}

extension VesperRuntimeDownloadEventList {
    func decodePublicEvents() -> [VesperDownloadEvent]? {
        decodeRuntimeRecords(events, count: len) { event in
            switch event.kind {
            case .created:
                guard let task = event.task,
                      let decodedTask = task.pointee.decodePublic()
                else {
                    return nil
                }
                return .created(decodedTask)
            case .stateChanged:
                guard event.task_id != 0,
                      let state = VesperDownloadState(
                    rawValue: Int(event.state_status.rawValue)
                ) else {
                    return nil
                }
                let error: VesperDownloadError?
                if event.state_has_error {
                    guard let decodedError = event.decodePublicError() else {
                        return nil
                    }
                    error = decodedError
                } else {
                    error = nil
                }
                return .stateChanged(
                    VesperDownloadTaskStatePatch(
                        taskId: event.task_id,
                        state: state,
                        progress: event.state_progress.toPublic(),
                        error: error,
                        completedPath: stringFromRuntimeCString(event.state_completed_path)
                    )
                )
            case .assetIndexUpdated:
                guard let task = event.task,
                      let decodedTask = task.pointee.decodePublic()
                else {
                    return nil
                }
                return .assetIndexUpdated(decodedTask)
            case .progressUpdated:
                guard event.task_id != 0 else {
                    return nil
                }
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
    func decodePublicError() -> VesperDownloadError? {
        guard let message = stringFromRuntimeCString(state_error_message) else {
            return nil
        }
        return VesperDownloadError(
            code: VesperPlayerErrorCode(ffiCode: state_error_code),
            category: VesperPlayerErrorCategory(ffiCategory: state_error_category),
            retriable: state_error_retriable,
            message: message
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
