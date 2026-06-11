import Foundation

extension VesperForegroundDownloadExecutor {
    func prepareAssetIndex(task: VesperDownloadTaskSnapshot) async throws -> VesperDownloadAssetIndex {
        let requestHeaders = task.source.source.headers
        if !task.assetIndex.resources.isEmpty || !task.assetIndex.segments.isEmpty {
            return try await completePreparedAssetIndex(
                contentFormat: task.source.contentFormat,
                assetIndex: task.assetIndex,
                requestHeaders: requestHeaders
            )
        }

        switch task.source.contentFormat {
        case .hlsSegments:
            return try await planHlsAssetIndex(task: task, requestHeaders: requestHeaders)
        case .dashSegments:
            return try await planDashAssetIndex(task: task, requestHeaders: requestHeaders)
        case .flvSegments:
            return try await planFlvAssetIndex(task: task, requestHeaders: requestHeaders)
        case .singleFile:
            return try await planSingleFileAssetIndex(task: task, requestHeaders: requestHeaders)
        case .unknown:
            throw VesperForegroundDownloadPreparationError.unsupported("download preparation cannot plan an unknown content format")
        }
    }

    func completePreparedAssetIndex(
        contentFormat: VesperDownloadContentFormat,
        assetIndex: VesperDownloadAssetIndex,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        var totalSizeBytes: UInt64 = 0
        var resources: [VesperDownloadResourceRecord] = []
        resources.reserveCapacity(assetIndex.resources.count)

        for resource in assetIndex.resources {
            if resource.generatedText != nil {
                resources.append(resource)
                continue
            }
            let sizeBytes: UInt64
            if let existingSizeBytes = resource.sizeBytes {
                sizeBytes = existingSizeBytes
            } else {
                sizeBytes = try await probeRequiredSize(resource.uri, byteRange: resource.byteRange, requestHeaders: requestHeaders)
            }
            totalSizeBytes += sizeBytes
            resources.append(resource.withSizeBytes(sizeBytes))
        }

        var segments: [VesperDownloadSegmentRecord] = []
        segments.reserveCapacity(assetIndex.segments.count)
        for segment in assetIndex.segments {
            let sizeBytes: UInt64
            if let existingSizeBytes = segment.sizeBytes {
                sizeBytes = existingSizeBytes
            } else {
                sizeBytes = try await probeRequiredSize(segment.uri, byteRange: segment.byteRange, requestHeaders: requestHeaders)
            }
            totalSizeBytes += sizeBytes
            segments.append(segment.withSizeBytes(sizeBytes))
        }

        return VesperDownloadAssetIndex(
            contentFormat: contentFormat,
            version: assetIndex.version,
            etag: assetIndex.etag,
            checksum: assetIndex.checksum,
            totalSizeBytes: assetIndex.totalSizeBytes ?? totalSizeBytes,
            resources: resources,
            segments: segments,
            completedPath: assetIndex.completedPath
        )
    }

    func planSingleFileAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let uri = task.source.manifestUri ?? task.source.source.uri
        let sizeBytes = try await probeRequiredSize(uri, byteRange: nil, requestHeaders: requestHeaders)
        return VesperDownloadAssetIndex(
            contentFormat: .singleFile,
            totalSizeBytes: sizeBytes,
            resources: [
                VesperDownloadResourceRecord(
                    resourceId: "single-file",
                    uri: uri,
                    relativePath: inferredFileName(uri),
                    sizeBytes: sizeBytes
                )
            ]
        )
    }

    func planHlsAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let manifestUri = task.source.manifestUri ?? task.source.source.uri
        let manifestText = try await fetchText(manifestUri, requestHeaders: requestHeaders)
        if manifestText.range(of: "#EXT-X-STREAM-INF", options: .caseInsensitive) != nil {
            return try await planHlsMasterAssetIndex(
                manifestUri: manifestUri,
                manifestText: manifestText,
                profile: task.profile,
                requestHeaders: requestHeaders
            )
        }

        let media = try parseHlsMediaPlaylist(playlistUri: manifestUri, playlistText: manifestText)
        return try await buildHlsMediaAssetIndex(
            manifestPath: "index.m3u8",
            mediaPlaylists: [("media", media)],
            requestHeaders: requestHeaders
        )
    }

    func planHlsMasterAssetIndex(
        manifestUri: String,
        manifestText: String,
        profile: VesperDownloadProfile,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let master = parseHlsMasterPlaylist(manifestUri: manifestUri, manifestText: manifestText)
        guard
            let variant = profile.variantId.flatMap({ variantId in
                master.variants.first { $0.uri == variantId || $0.attributes["NAME"] == variantId }
            }) ?? master.variants.first
        else {
            throw VesperForegroundDownloadPreparationError.invalidSource("HLS master playlist did not contain a playable variant")
        }

        var mediaPlaylists: [(String, HlsMediaPlaylist)] = [
            (
                "video",
                try parseHlsMediaPlaylist(
                    playlistUri: variant.uri,
                    playlistText: try await fetchText(variant.uri, requestHeaders: requestHeaders)
                )
            )
        ]

        let audio = profile.preferredAudioLanguage.flatMap { language in
            master.audio.first { $0.attributes["LANGUAGE"]?.caseInsensitiveCompare(language) == .orderedSame }
        } ?? master.audio.first { $0.attributes["DEFAULT"]?.caseInsensitiveCompare("YES") == .orderedSame }
            ?? master.audio.first
        if let audio {
            mediaPlaylists.append(
                (
                    "audio",
                    try parseHlsMediaPlaylist(
                        playlistUri: audio.uri,
                        playlistText: try await fetchText(audio.uri, requestHeaders: requestHeaders)
                    )
                )
            )
        }

        let planned = try await buildHlsMediaAssetIndex(
            manifestPath: "index.m3u8",
            mediaPlaylists: mediaPlaylists,
            requestHeaders: requestHeaders
        )
        let mediaResourceNames = planned.resources.compactMap { resource -> String? in
            guard
                let relativePath = resource.relativePath,
                relativePath.hasSuffix(".m3u8"),
                relativePath != "index.m3u8"
            else {
                return nil
            }
            return URL(fileURLWithPath: relativePath).lastPathComponent
        }
        let masterText = rewriteHlsMaster(
            variantAttributes: variant.attributes,
            mediaResourceNames: mediaResourceNames
        )
        return planned.withResources(
            planned.resources.map { resource in
                resource.resourceId == "hls-master"
                    ? resource.withGeneratedText(masterText)
                    : resource
            }
        )
    }

    func buildHlsMediaAssetIndex(
        manifestPath: String,
        mediaPlaylists: [(String, HlsMediaPlaylist)],
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        var resources = [
            VesperDownloadResourceRecord(
                resourceId: "hls-master",
                uri: "vesper-generated://hls/\(manifestPath)",
                relativePath: manifestPath
            )
        ]
        var segments: [VesperDownloadSegmentRecord] = []
        var seenMaps = Set<String>()
        var totalSizeBytes: UInt64 = 0

        for (mediaId, playlist) in mediaPlaylists {
            let playlistPath =
                mediaPlaylists.count == 1 && manifestPath == "index.m3u8"
                    ? "index.m3u8"
                    : "\(mediaId).m3u8"
            var localMaps: [String: String] = [:]

            for (index, map) in playlist.maps.enumerated() {
                let key = hlsByteRangeKey(uri: map.uri, byteRange: map.byteRange)
                if seenMaps.insert(key).inserted {
                    let sizeBytes = try await probeRequiredSize(map.uri, byteRange: map.byteRange, requestHeaders: requestHeaders)
                    totalSizeBytes += sizeBytes
                    let relativePath = "segments/\(mediaId)-init-\(index).\(extensionFromUri(map.uri, fallback: "mp4"))"
                    resources.append(
                        VesperDownloadResourceRecord(
                            resourceId: "hls-\(mediaId)-init-\(index)",
                            uri: map.uri,
                            relativePath: relativePath,
                            byteRange: map.byteRange,
                            sizeBytes: sizeBytes
                        )
                    )
                    localMaps[key] = relativePath
                }
            }

            for segment in playlist.segments {
                let sizeBytes = try await probeRequiredSize(segment.uri, byteRange: segment.byteRange, requestHeaders: requestHeaders)
                totalSizeBytes += sizeBytes
                segments.append(
                    VesperDownloadSegmentRecord(
                        segmentId: "hls-\(mediaId)-\(segment.sequence)",
                        uri: segment.uri,
                        relativePath: "segments/\(mediaId)-\(padded(segment.sequence, width: 5)).\(extensionFromUri(segment.uri, fallback: "ts"))",
                        sequence: segment.sequence,
                        byteRange: segment.byteRange,
                        sizeBytes: sizeBytes
                    )
                )
            }

            resources.append(
                VesperDownloadResourceRecord(
                    resourceId: "hls-\(mediaId)-playlist",
                    uri: "vesper-generated://hls/\(playlistPath)",
                    relativePath: playlistPath,
                    generatedText: rewriteHlsMedia(mediaId: mediaId, playlist: playlist, localMaps: localMaps)
                )
            )
        }

        if mediaPlaylists.count == 1,
           let mediaResourceIndex = resources.firstIndex(where: { $0.resourceId.hasSuffix("-playlist") }) {
            let mediaResource = resources.remove(at: mediaResourceIndex)
            resources[0] = resources[0].withGeneratedText(mediaResource.generatedText ?? "")
        }

        return VesperDownloadAssetIndex(
            contentFormat: .hlsSegments,
            totalSizeBytes: totalSizeBytes,
            resources: resources,
            segments: segments
        )
    }

    func planDashAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let manifestUri = task.source.manifestUri ?? task.source.source.uri
        let manifestText = try await fetchText(manifestUri, requestHeaders: requestHeaders)
        let documentType = xmlAttr(manifestText, tag: "MPD", attr: "type")
        if let documentType, !documentType.isEmpty, documentType.caseInsensitiveCompare("static") != .orderedSame {
            throw VesperForegroundDownloadPreparationError.unsupported("DASH download preparation requires a static MPD")
        }
        guard let durationSeconds = parseIso8601DurationSeconds(xmlAttr(manifestText, tag: "MPD", attr: "mediaPresentationDuration")) else {
            throw VesperForegroundDownloadPreparationError.invalidSource("DASH SegmentTemplate planning requires a finite MPD duration")
        }

        let representations = selectDashRepresentations(
            manifestText: manifestText,
            manifestUri: manifestUri,
            profile: task.profile
        )
        if representations.isEmpty {
            throw VesperForegroundDownloadPreparationError.invalidSource("DASH MPD did not contain a supported SegmentTemplate or SegmentBase representation")
        }

        var resources: [VesperDownloadResourceRecord] = []
        var segments: [VesperDownloadSegmentRecord] = []
        var rewrittenAdaptationSets: [String] = []
        var totalSizeBytes: UInt64 = 0
        var globalSequence: UInt64 = 1

        for (index, representation) in representations.enumerated() {
            let mediaId = representation.mediaId.isEmpty ? "media\(index)" : representation.mediaId
            if let template = representation.template {
                guard template.duration > 0 else {
                    throw VesperForegroundDownloadPreparationError.invalidSource("DASH SegmentTemplate duration must be greater than zero")
                }
                let segmentSeconds = Double(template.duration) / Double(max(template.timescale, 1))
                let segmentCount = max(UInt64(ceil(durationSeconds / segmentSeconds)), 1)
                if let initialization = template.initialization, !initialization.isEmpty {
                    let remote = resolveRemoteReference(
                        baseUri: representation.baseUri,
                        reference: expandDashTemplate(initialization, representationId: representation.id, number: template.startNumber)
                    )
                    let sizeBytes = try await probeRequiredSize(remote, byteRange: nil, requestHeaders: requestHeaders)
                    totalSizeBytes += sizeBytes
                    resources.append(
                        VesperDownloadResourceRecord(
                            resourceId: "dash-\(mediaId)-init",
                            uri: remote,
                            relativePath: "segments/\(mediaId)-init.mp4",
                            sizeBytes: sizeBytes
                        )
                    )
                }

                for offset in 0..<segmentCount {
                    let number = template.startNumber + offset
                    let remote = resolveRemoteReference(
                        baseUri: representation.baseUri,
                        reference: expandDashTemplate(template.media, representationId: representation.id, number: number)
                    )
                    let sizeBytes = try await probeRequiredSize(remote, byteRange: nil, requestHeaders: requestHeaders)
                    totalSizeBytes += sizeBytes
                    segments.append(
                        VesperDownloadSegmentRecord(
                            segmentId: "dash-\(mediaId)-segment-\(number)",
                            uri: remote,
                            relativePath: "segments/\(mediaId)-\(number).m4s",
                            sequence: globalSequence,
                            sizeBytes: sizeBytes
                        )
                    )
                    globalSequence += 1
                }

                rewrittenAdaptationSets.append(
                    rewriteDashTemplateAdaptationSet(
                        representation: representation,
                        template: template,
                        mediaId: mediaId,
                        segmentCount: segmentCount
                    )
                )
            } else if let baseUrl = representation.baseUrl {
                let remote = resolveRemoteReference(baseUri: representation.baseUri, reference: baseUrl)
                let sizeBytes = try await probeRequiredSize(remote, byteRange: nil, requestHeaders: requestHeaders)
                totalSizeBytes += sizeBytes
                let localName = "media-\(mediaId).\(extensionFromUri(remote, fallback: "mp4"))"
                resources.append(
                    VesperDownloadResourceRecord(
                        resourceId: "dash-\(mediaId)-media",
                        uri: remote,
                        relativePath: localName,
                        sizeBytes: sizeBytes
                    )
                )
                rewrittenAdaptationSets.append(
                    rewriteDashSegmentBaseAdaptationSet(representation: representation, localName: localName)
                )
            }
        }

        resources.insert(
            VesperDownloadResourceRecord(
                resourceId: "dash-manifest",
                uri: "vesper-generated://dash/manifest.mpd",
                relativePath: "manifest.mpd",
                generatedText: rewriteDashMpd(
                    duration: xmlAttr(manifestText, tag: "MPD", attr: "mediaPresentationDuration"),
                    adaptationSets: rewrittenAdaptationSets
                )
            ),
            at: 0
        )

        return VesperDownloadAssetIndex(
            contentFormat: .dashSegments,
            totalSizeBytes: totalSizeBytes,
            resources: resources,
            segments: segments
        )
    }

    func planFlvAssetIndex(
        task: VesperDownloadTaskSnapshot,
        requestHeaders: [String: String]
    ) async throws -> VesperDownloadAssetIndex {
        let uri = task.source.manifestUri ?? task.source.source.uri
        let clipUris =
            extensionFromUri(uri, fallback: "flv").caseInsensitiveCompare("flv") == .orderedSame
                ? [uri]
                : parseFlvClipManifest(baseUri: uri, manifestText: try await fetchText(uri, requestHeaders: requestHeaders))
        if clipUris.isEmpty {
            throw VesperForegroundDownloadPreparationError.invalidSource("FLV clip manifest did not contain any clip URI")
        }

        var totalSizeBytes: UInt64 = 0
        var concat = "ffconcat version 1.0\n"
        var segments: [VesperDownloadSegmentRecord] = []
        for (index, clipUri) in clipUris.enumerated() {
            let sequence = UInt64(index + 1)
            let sizeBytes = try await probeRequiredSize(clipUri, byteRange: nil, requestHeaders: requestHeaders)
            totalSizeBytes += sizeBytes
            let localPath = "clips/clip-\(padded(sequence, width: 5)).\(extensionFromUri(clipUri, fallback: "flv"))"
            concat += "file '\(escapeFfconcatPath(localPath))'\n"
            segments.append(
                VesperDownloadSegmentRecord(
                    segmentId: "flv-clip-\(sequence)",
                    uri: clipUri,
                    relativePath: localPath,
                    sequence: sequence,
                    sizeBytes: sizeBytes
                )
            )
        }

        return VesperDownloadAssetIndex(
            contentFormat: .flvSegments,
            totalSizeBytes: totalSizeBytes,
            resources: [
                VesperDownloadResourceRecord(
                    resourceId: "flv-concat",
                    uri: "vesper-generated://flv/manifest.ffconcat",
                    relativePath: "manifest.ffconcat",
                    generatedText: concat
                )
            ],
            segments: segments
        )
    }
}
