import Flutter
import Foundation
import VesperPlayerKit

extension TimelineUiState {
    func toMap() -> [String: Any] {
        [
            "kind": kind.toWireName(),
            "isSeekable": isSeekable,
            "seekableRange": flutterValue(seekableRange.map {
                [
                    "startMs": $0.startMs,
                    "endMs": $0.endMs,
                ]
            }),
            "liveEdgeMs": flutterValue(liveEdgeMs),
            "positionMs": positionMs,
            "durationMs": flutterValue(durationMs),
        ]
    }
}

extension VesperTrackCatalog {
    func toMap() -> [String: Any] {
        [
            "tracks": tracks.map(\.toMap),
            "adaptiveVideo": adaptiveVideo,
            "adaptiveAudio": adaptiveAudio,
        ]
    }
}

extension VesperMediaTrack {
    var toMap: [String: Any] {
        [
            "id": id,
            "kind": kind.toWireName(),
            "label": flutterValue(label),
            "language": flutterValue(language),
            "codec": flutterValue(codec),
            "bitRate": flutterValue(bitRate),
            "width": flutterValue(width),
            "height": flutterValue(height),
            "frameRate": flutterValue(frameRate),
            "channels": flutterValue(channels),
            "sampleRate": flutterValue(sampleRate),
            "isDefault": isDefault,
            "isForced": isForced,
        ]
    }
}

extension VesperTrackSelectionSnapshot {
    func toMap() -> [String: Any] {
        [
            "video": video.toMap(),
            "audio": audio.toMap(),
            "subtitle": subtitle.toMap(),
            "abrPolicy": abrPolicy.toMap(),
        ]
    }
}

extension VesperTrackSelection {
    func toMap() -> [String: Any] {
        [
            "mode": mode.toWireName(),
            "trackId": flutterValue(trackId),
        ]
    }
}

extension VesperAbrPolicy {
    func toMap() -> [String: Any] {
        [
            "mode": mode.toWireName(),
            "trackId": flutterValue(trackId),
            "maxBitRate": flutterValue(maxBitRate),
            "maxWidth": flutterValue(maxWidth),
            "maxHeight": flutterValue(maxHeight),
        ]
    }
}

extension VesperPlaybackResiliencePolicy {
    func toMap() -> [String: Any] {
        [
            "buffering": buffering.toMap(),
            "retry": retry.toMap(),
            "cache": cache.toMap(),
        ]
    }
}

extension VesperBufferingPolicy {
    func toMap() -> [String: Any] {
        [
            "preset": preset.toWireName(),
            "minBufferMs": flutterValue(minBufferMs),
            "maxBufferMs": flutterValue(maxBufferMs),
            "bufferForPlaybackMs": flutterValue(bufferForPlaybackMs),
            "bufferForPlaybackAfterRebufferMs": flutterValue(bufferForPlaybackAfterRebufferMs),
        ]
    }
}

extension VesperRetryPolicy {
    func toMap() -> [String: Any] {
        [
            "maxAttempts": flutterValue(maxAttempts),
            "baseDelayMs": baseDelayMs,
            "maxDelayMs": maxDelayMs,
            "backoff": backoff.toWireName(),
        ]
    }
}

extension VesperCachePolicy {
    func toMap() -> [String: Any] {
        [
            "preset": preset.toWireName(),
            "maxMemoryBytes": flutterValue(maxMemoryBytes),
            "maxDiskBytes": flutterValue(maxDiskBytes),
        ]
    }
}

extension VesperPlayerSource {
    func toMap() -> [String: Any] {
        [
            "uri": uri,
            "label": label,
            "kind": kind.rawValue,
            "protocol": `protocol`.rawValue,
            "headers": headers,
            "drmConfiguration": flutterValue(drmConfiguration?.toMap()),
        ]
    }
}

extension VesperPlayerDrmConfiguration {
    func toMap() -> [String: Any] {
        [
            "keySystem": keySystem,
            "licenseUri": licenseUri,
            "licenseHeaders": licenseHeaders,
            "fairPlayCertificateUri": flutterValue(fairPlayCertificateUri),
            "fairPlayCertificateBase64": flutterValue(fairPlayCertificateBase64),
            "multiSession": multiSession,
        ]
    }
}

extension VesperDownloadTaskSnapshot {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "assetId": assetId,
            "source": source.toMap,
            "profile": profile.toMap,
            "state": state.toWireName(),
            "progress": progress.toMap,
            "assetIndex": assetIndex.toMap,
            "error": flutterValue(error?.toMap),
        ]
    }
}

extension VesperDownloadTaskStatePatch {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "state": state.toWireName(),
            "progress": progress.toMap,
            "error": flutterValue(error?.toMap),
            "completedPath": flutterValue(completedPath),
        ]
    }
}

extension VesperDownloadTaskProgressPatch {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "progress": progress.toMap,
        ]
    }
}

extension VesperDownloadSource {
    var toMap: [String: Any] {
        [
            "source": source.toMap(),
            "contentFormat": contentFormat.toWireName(),
            "manifestUri": flutterValue(manifestUri),
        ]
    }
}

extension VesperDownloadProfile {
    var toMap: [String: Any] {
        [
            "variantId": flutterValue(variantId),
            "preferredAudioLanguage": flutterValue(preferredAudioLanguage),
            "preferredSubtitleLanguage": flutterValue(preferredSubtitleLanguage),
            "selectedTrackIds": selectedTrackIds,
            "targetOutputFormat": flutterValue(targetOutputFormat?.toWireName()),
            "targetDirectory": flutterValue(targetDirectory?.path),
            "allowMeteredNetwork": allowMeteredNetwork,
        ]
    }
}

extension VesperDownloadProgressSnapshot {
    var toMap: [String: Any] {
        [
            "receivedBytes": receivedBytes,
            "totalBytes": flutterValue(totalBytes),
            "receivedSegments": receivedSegments,
            "totalSegments": flutterValue(totalSegments),
        ]
    }
}

extension VesperDownloadAssetIndex {
    var toMap: [String: Any] {
        [
            "contentFormat": contentFormat.toWireName(),
            "version": flutterValue(version),
            "etag": flutterValue(etag),
            "checksum": flutterValue(checksum),
            "totalSizeBytes": flutterValue(totalSizeBytes),
            "resources": resources.map(\.toMap),
            "segments": segments.map(\.toMap),
            "streams": streams.map(\.toMap),
            "completedPath": flutterValue(completedPath),
        ]
    }
}

extension VesperDownloadResourceRecord {
    var toMap: [String: Any] {
        [
            "resourceId": resourceId,
            "uri": uri,
            "relativePath": flutterValue(relativePath),
            "byteRange": flutterValue(byteRange?.toMap),
            "generatedText": NSNull(),
            "sizeBytes": flutterValue(sizeBytes),
            "etag": flutterValue(etag),
            "checksum": flutterValue(checksum),
        ]
    }
}

extension VesperDownloadStaleResource {
    var toMap: [String: Any] {
        [
            "taskId": taskId,
            "resourceId": flutterValue(resourceId),
            "segmentId": flutterValue(segmentId),
            "uri": flutterValue(uri),
            "phase": phase == .download ? "download" : "prepare",
            "statusCode": flutterValue(statusCode),
            "receivedBytes": receivedBytes,
            "message": message,
        ]
    }
}

extension VesperDownloadSegmentRecord {
    var toMap: [String: Any] {
        [
            "segmentId": segmentId,
            "uri": uri,
            "relativePath": flutterValue(relativePath),
            "sequence": flutterValue(sequence),
            "byteRange": flutterValue(byteRange?.toMap),
            "sizeBytes": flutterValue(sizeBytes),
            "checksum": flutterValue(checksum),
        ]
    }
}

extension VesperDownloadAssetStream {
    var toMap: [String: Any] {
        [
            "streamId": streamId,
            "kind": kind.toWireName(),
            "language": flutterValue(language),
            "codec": flutterValue(codec),
            "label": flutterValue(label),
            "qualityRank": flutterValue(qualityRank),
            "resourceIds": resourceIds,
            "segmentIds": segmentIds,
            "metadata": metadata,
        ]
    }
}

extension VesperDownloadByteRange {
    var toMap: [String: Any] {
        [
            "offset": offset,
            "length": length,
        ]
    }
}

extension VesperDownloadError {
    var toMap: [String: Any] {
        [
            "code": code.rawValue,
            "category": category.rawValue,
            "retriable": retriable,
            "message": message,
        ]
    }
}

extension VesperPlayerError {
    var toMap: [String: Any] {
        var mappedDetails: [String: Any] = details
        if mappedDetails["hdrMetadata"] == nil,
           let hdrMetadata = flutterHdrMetadataMap(fromErrorDetails: self.details) {
            mappedDetails["hdrMetadata"] = hdrMetadata
        }
        return [
            "message": message,
            "code": code.rawValue,
            "category": category.rawValue,
            "retriable": retriable,
            "details": mappedDetails,
        ]
    }
}

func flutterHdrMetadataMap(fromErrorDetails details: [String: String]) -> [String: Any]? {
    var values: [String: Any] = [:]
    if let value = details.stringValue("hdrKind") {
        values["hdrKind"] = value
    }
    if let value = details.stringValue("dolbyVisionMode") {
        values["dolbyVisionMode"] = value
    }
    if let value = details.firstString(
        "hdrMetadataProbe",
        "runtimeFormatHdrMetadataProbe",
        "assetVideoHdrMetadataProbe",
        "assetProbe"
    ) {
        values["probe"] = value
    }
    if let value = details.firstString("assetVideoCodec", "runtimeFormatCodecs") {
        values["codec"] = value
    }
    if let value = details.stringValue("runtimeFormatSampleMimeType") {
        values["sampleMimeType"] = value
    }
    if let value = details.stringValue("assetVideoColorPrimaries") {
        values["colorPrimaries"] = value
    }
    if let value = details.stringValue("runtimeFormatColorSpace") {
        values["colorSpace"] = value
    }
    if let value = details.stringValue("runtimeFormatColorRange") {
        values["colorRange"] = value
    }
    if let value = details.firstString("assetVideoTransferFunction", "runtimeFormatColorTransfer") {
        values["transferFunction"] = value
    }
    if let value = details.stringValue("assetVideoYCbCrMatrix") {
        values["yCbCrMatrix"] = value
    }
    if let value = details.stringValue("assetVideoAlternativeTransferCharacteristics") {
        values["alternativeTransferCharacteristics"] = value
    }
    if let value = details.intValue("runtimeFormatLumaBitDepth") {
        values["lumaBitDepth"] = value
    }
    if let value = details.intValue("runtimeFormatChromaBitDepth") {
        values["chromaBitDepth"] = value
    }
    if let value = details.boolValue("runtimeFormatHdrStaticInfoPresent") {
        values["hdrStaticInfoPresent"] = value
    }
    if let value = details.intValue("runtimeFormatHdrStaticInfoByteLength") {
        values["hdrStaticInfoByteLength"] = value
    }
    if let value = details.stringValue("runtimeFormatHdrStaticInfoParseError") {
        values["hdrStaticInfoParseError"] = value
    }
    if let value = details.firstInt("assetVideoMaxContentLightLevelNits", "runtimeFormatMaxContentLightLevelNits") {
        values["maxContentLightLevelNits"] = value
    }
    if let value = details.firstInt(
        "assetVideoMaxFrameAverageLightLevelNits",
        "runtimeFormatMaxFrameAverageLightLevelNits"
    ) {
        values["maxFrameAverageLightLevelNits"] = value
    }
    if let value = details.boolValue("assetVideoMasteringDisplayColorVolumePresent") {
        values["masteringDisplayColorVolumePresent"] = value
    }
    if let value = details.intValue("assetVideoMasteringDisplayColorVolumeByteLength") {
        values["masteringDisplayColorVolumeByteLength"] = value
    }
    if let value = details.stringValue("assetVideoMasteringDisplayColorVolumeParseError") {
        values["masteringDisplayColorVolumeParseError"] = value
    }
    if let value = details.chromaticityPoint("assetVideoMasteringDisplayPrimary0") {
        values["masteringDisplayPrimary0"] = value
    }
    if let value = details.chromaticityPoint("assetVideoMasteringDisplayPrimary1") {
        values["masteringDisplayPrimary1"] = value
    }
    if let value = details.chromaticityPoint("assetVideoMasteringDisplayPrimary2") {
        values["masteringDisplayPrimary2"] = value
    }
    if let value = details.chromaticityPoint("assetVideoMasteringDisplayWhitePoint") {
        values["masteringDisplayWhitePoint"] = value
    }
    if let value = details.doubleValue("assetVideoMasteringDisplayMaxLuminanceNits") {
        values["masteringDisplayMaxLuminanceNits"] = value
    }
    if let value = details.doubleValue("assetVideoMasteringDisplayMinLuminanceNits") {
        values["masteringDisplayMinLuminanceNits"] = value
    }
    if let value = details.stringValue("dolbyVisionCodec") {
        values["dolbyVisionCodec"] = value
    }
    if let value = details.intValue("dolbyVisionProfile") {
        values["dolbyVisionProfile"] = value
    }
    if let value = details.intValue("dolbyVisionLevel") {
        values["dolbyVisionLevel"] = value
    }
    if let value = details.stringValue("dolbyVisionCompatibility") {
        values["dolbyVisionCompatibility"] = value
    }
    if let value = details.stringValue("dolbyVisionProfileFamily") {
        values["dolbyVisionProfileFamily"] = value
    }
    if let value = details.stringValue("dolbyVisionBaseLayer") {
        values["dolbyVisionBaseLayer"] = value
    }
    if let value = details.stringValue("dolbyVisionFallbackTarget") {
        values["dolbyVisionFallbackTarget"] = value
    }
    if let value = details.stringValue("dolbyVisionBaseLayerEvidence") {
        values["dolbyVisionBaseLayerEvidence"] = value
    }
    if let value = details.stringValue("dolbyVisionBaseLayerTransferFunction") {
        values["dolbyVisionBaseLayerTransferFunction"] = value
    }
    return values.isEmpty ? nil : values
}

func flutterPlaybackCapabilityResultMap(
    _ result: VesperPlaybackCapabilityProbeResult
) -> [String: Any] {
    var map = result.wireMap
    map["hdrMetadata"] = flutterHdrMetadataMap(from: result) ?? NSNull()
    return map
}

func flutterHdrMetadataMap(
    from result: VesperPlaybackCapabilityProbeResult
) -> [String: Any]? {
    var values = result.hdrMetadata?.flutterMap ?? [:]
    if values["hdrKind"] == nil, result.hdrKind != .none, result.hdrKind != .unknown {
        values["hdrKind"] = result.hdrKind.rawValue
    }
    if values["dolbyVisionMode"] == nil, result.dolbyVisionMode != .none {
        values["dolbyVisionMode"] = result.dolbyVisionMode.rawValue
    }
    if let value = result.diagnostics.firstString(
        "runtimeFormatHdrMetadataProbe",
        "assetVideoHdrMetadataProbe",
        "assetProbe"
    ) {
        values["probe"] = value
    }
    if let value = result.diagnostics.firstString("assetVideoCodec", "runtimeFormatCodecs") {
        values["codec"] = value
    }
    if let value = result.diagnostics.stringValue("runtimeFormatSampleMimeType") {
        values["sampleMimeType"] = value
    }
    if let value = result.diagnostics.stringValue("assetVideoColorPrimaries") {
        values["colorPrimaries"] = value
    }
    if let value = result.diagnostics.stringValue("runtimeFormatColorSpace") {
        values["colorSpace"] = value
    }
    if let value = result.diagnostics.stringValue("runtimeFormatColorRange") {
        values["colorRange"] = value
    }
    if let value = result.diagnostics.firstString(
        "assetVideoTransferFunction",
        "runtimeFormatColorTransfer"
    ) {
        values["transferFunction"] = value
    }
    if let value = result.diagnostics.stringValue("assetVideoYCbCrMatrix") {
        values["yCbCrMatrix"] = value
    }
    if let value = result.diagnostics.stringValue("assetVideoAlternativeTransferCharacteristics") {
        values["alternativeTransferCharacteristics"] = value
    }
    if let value = result.diagnostics.intValue("runtimeFormatLumaBitDepth") {
        values["lumaBitDepth"] = value
    }
    if let value = result.diagnostics.intValue("runtimeFormatChromaBitDepth") {
        values["chromaBitDepth"] = value
    }
    if let value = result.diagnostics.boolValue("runtimeFormatHdrStaticInfoPresent") {
        values["hdrStaticInfoPresent"] = value
    }
    if let value = result.diagnostics.intValue("runtimeFormatHdrStaticInfoByteLength") {
        values["hdrStaticInfoByteLength"] = value
    }
    if let value = result.diagnostics.stringValue("runtimeFormatHdrStaticInfoParseError") {
        values["hdrStaticInfoParseError"] = value
    }
    if let value = result.diagnostics.firstInt(
        "assetVideoMaxContentLightLevelNits",
        "runtimeFormatMaxContentLightLevelNits"
    ) {
        values["maxContentLightLevelNits"] = value
    }
    if let value = result.diagnostics.firstInt(
        "assetVideoMaxFrameAverageLightLevelNits",
        "runtimeFormatMaxFrameAverageLightLevelNits"
    ) {
        values["maxFrameAverageLightLevelNits"] = value
    }
    if let value = result.diagnostics.boolValue("assetVideoMasteringDisplayColorVolumePresent") {
        values["masteringDisplayColorVolumePresent"] = value
    }
    if let value = result.diagnostics.intValue("assetVideoMasteringDisplayColorVolumeByteLength") {
        values["masteringDisplayColorVolumeByteLength"] = value
    }
    if let value = result.diagnostics.stringValue("assetVideoMasteringDisplayColorVolumeParseError") {
        values["masteringDisplayColorVolumeParseError"] = value
    }
    if let value = result.diagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary0") {
        values["masteringDisplayPrimary0"] = value
    }
    if let value = result.diagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary1") {
        values["masteringDisplayPrimary1"] = value
    }
    if let value = result.diagnostics.chromaticityPoint("assetVideoMasteringDisplayPrimary2") {
        values["masteringDisplayPrimary2"] = value
    }
    if let value = result.diagnostics.chromaticityPoint("assetVideoMasteringDisplayWhitePoint") {
        values["masteringDisplayWhitePoint"] = value
    }
    if let value = result.diagnostics.doubleValue("assetVideoMasteringDisplayMaxLuminanceNits") {
        values["masteringDisplayMaxLuminanceNits"] = value
    }
    if let value = result.diagnostics.doubleValue("assetVideoMasteringDisplayMinLuminanceNits") {
        values["masteringDisplayMinLuminanceNits"] = value
    }
    if let value = result.diagnostics.stringValue("dolbyVisionCodec") {
        values["dolbyVisionCodec"] = value
    }
    if let value = result.diagnostics.intValue("dolbyVisionProfile") {
        values["dolbyVisionProfile"] = value
    }
    if let value = result.diagnostics.intValue("dolbyVisionLevel") {
        values["dolbyVisionLevel"] = value
    }
    if let value = result.diagnostics.stringValue("dolbyVisionCompatibility") {
        values["dolbyVisionCompatibility"] = value
    }
    if let value = result.diagnostics.stringValue("dolbyVisionProfileFamily") {
        values["dolbyVisionProfileFamily"] = value
    }
    if let value = result.diagnostics.stringValue("dolbyVisionBaseLayer") {
        values["dolbyVisionBaseLayer"] = value
    }
    if let value = result.diagnostics.stringValue("dolbyVisionFallbackTarget") {
        values["dolbyVisionFallbackTarget"] = value
    }
    if let value = result.diagnostics.stringValue("dolbyVisionBaseLayerEvidence") {
        values["dolbyVisionBaseLayerEvidence"] = value
    }
    if let value = result.diagnostics.stringValue("dolbyVisionBaseLayerTransferFunction") {
        values["dolbyVisionBaseLayerTransferFunction"] = value
    }
    return values.isEmpty ? nil : values
}

private extension VesperPlaybackCapabilityHdrMetadata {
    var flutterMap: [String: Any] {
        var values: [String: Any] = [:]
        if let hdrKind {
            values["hdrKind"] = hdrKind.rawValue
        }
        if let dolbyVisionMode {
            values["dolbyVisionMode"] = dolbyVisionMode.rawValue
        }
        if let probe {
            values["probe"] = probe
        }
        if let codec {
            values["codec"] = codec
        }
        if let sampleMimeType {
            values["sampleMimeType"] = sampleMimeType
        }
        if let colorPrimaries {
            values["colorPrimaries"] = colorPrimaries
        }
        if let colorSpace {
            values["colorSpace"] = colorSpace
        }
        if let colorRange {
            values["colorRange"] = colorRange
        }
        if let transferFunction {
            values["transferFunction"] = transferFunction
        }
        if let yCbCrMatrix {
            values["yCbCrMatrix"] = yCbCrMatrix
        }
        if let alternativeTransferCharacteristics {
            values["alternativeTransferCharacteristics"] = alternativeTransferCharacteristics
        }
        if let lumaBitDepth {
            values["lumaBitDepth"] = lumaBitDepth
        }
        if let chromaBitDepth {
            values["chromaBitDepth"] = chromaBitDepth
        }
        if let hdrStaticInfoPresent {
            values["hdrStaticInfoPresent"] = hdrStaticInfoPresent
        }
        if let hdrStaticInfoByteLength {
            values["hdrStaticInfoByteLength"] = hdrStaticInfoByteLength
        }
        if let hdrStaticInfoParseError {
            values["hdrStaticInfoParseError"] = hdrStaticInfoParseError
        }
        if let maxContentLightLevelNits {
            values["maxContentLightLevelNits"] = maxContentLightLevelNits
        }
        if let maxFrameAverageLightLevelNits {
            values["maxFrameAverageLightLevelNits"] = maxFrameAverageLightLevelNits
        }
        if let masteringDisplayColorVolumePresent {
            values["masteringDisplayColorVolumePresent"] = masteringDisplayColorVolumePresent
        }
        if let masteringDisplayColorVolumeByteLength {
            values["masteringDisplayColorVolumeByteLength"] = masteringDisplayColorVolumeByteLength
        }
        if let masteringDisplayColorVolumeParseError {
            values["masteringDisplayColorVolumeParseError"] = masteringDisplayColorVolumeParseError
        }
        if let masteringDisplayPrimary0 {
            values["masteringDisplayPrimary0"] = masteringDisplayPrimary0.flutterMap
        }
        if let masteringDisplayPrimary1 {
            values["masteringDisplayPrimary1"] = masteringDisplayPrimary1.flutterMap
        }
        if let masteringDisplayPrimary2 {
            values["masteringDisplayPrimary2"] = masteringDisplayPrimary2.flutterMap
        }
        if let masteringDisplayWhitePoint {
            values["masteringDisplayWhitePoint"] = masteringDisplayWhitePoint.flutterMap
        }
        if let masteringDisplayMaxLuminanceNits {
            values["masteringDisplayMaxLuminanceNits"] = masteringDisplayMaxLuminanceNits
        }
        if let masteringDisplayMinLuminanceNits {
            values["masteringDisplayMinLuminanceNits"] = masteringDisplayMinLuminanceNits
        }
        if let dolbyVisionCodec {
            values["dolbyVisionCodec"] = dolbyVisionCodec
        }
        if let dolbyVisionProfile {
            values["dolbyVisionProfile"] = dolbyVisionProfile
        }
        if let dolbyVisionLevel {
            values["dolbyVisionLevel"] = dolbyVisionLevel
        }
        if let dolbyVisionCompatibility {
            values["dolbyVisionCompatibility"] = dolbyVisionCompatibility
        }
        if let dolbyVisionProfileFamily {
            values["dolbyVisionProfileFamily"] = dolbyVisionProfileFamily
        }
        if let dolbyVisionBaseLayer {
            values["dolbyVisionBaseLayer"] = dolbyVisionBaseLayer
        }
        if let dolbyVisionFallbackTarget {
            values["dolbyVisionFallbackTarget"] = dolbyVisionFallbackTarget
        }
        if let dolbyVisionBaseLayerEvidence {
            values["dolbyVisionBaseLayerEvidence"] = dolbyVisionBaseLayerEvidence
        }
        if let dolbyVisionBaseLayerTransferFunction {
            values["dolbyVisionBaseLayerTransferFunction"] = dolbyVisionBaseLayerTransferFunction
        }
        return values
    }
}

private extension VesperHdrChromaticityPoint {
    var flutterMap: [String: Double] {
        ["x": x, "y": y]
    }
}

private extension Dictionary where Key == String, Value == String {
    func firstString(_ keys: String...) -> String? {
        keys.compactMap { stringValue($0) }.first
    }

    func firstInt(_ keys: String...) -> Int? {
        keys.compactMap { intValue($0) }.first
    }

    func stringValue(_ key: String) -> String? {
        guard let value = self[key], !value.isEmpty else {
            return nil
        }
        return value
    }

    func boolValue(_ key: String) -> Bool? {
        guard let value = stringValue(key) else {
            return nil
        }
        switch value {
        case "true":
            return true
        case "false":
            return false
        default:
            return nil
        }
    }

    func intValue(_ key: String) -> Int? {
        guard let value = stringValue(key) else {
            return nil
        }
        return Int(value)
    }

    func doubleValue(_ key: String) -> Double? {
        guard let value = stringValue(key), let parsed = Double(value), parsed.isFinite else {
            return nil
        }
        return parsed
    }

    func chromaticityPoint(_ key: String) -> [String: Double]? {
        guard let value = stringValue(key) else {
            return nil
        }
        let parts = value.split(separator: ",")
        guard parts.count == 2,
              let x = Double(parts[0].trimmingCharacters(in: .whitespacesAndNewlines)),
              let y = Double(parts[1].trimmingCharacters(in: .whitespacesAndNewlines))
        else {
            return nil
        }
        return ["x": x, "y": y]
    }
}

func flutterValue(_ value: Any?) -> Any {
    value ?? NSNull()
}

func errorMap(from error: Error) -> [String: Any] {
    if let pictureInPictureError = error as? VesperIosPictureInPictureError {
        return pictureInPictureError.toMap()
    }
    if let drmError = error as? VesperPlayerDrmUnsupportedError {
        return [
            "message": drmError.localizedDescription,
            "code": "unsupported",
            "category": "capability",
            "retriable": false,
            "details": drmError.details.merging(
                ["exception": String(describing: type(of: error))]
            ) { current, _ in current },
        ]
    }
    let code: String
    let category: String
    if let pluginError = error as? PluginError {
        switch pluginError {
        case .invalidSource:
            code = "invalidSource"
            category = "source"
        case .invalidTrackSelection, .invalidAbrPolicy:
            code = "unsupported"
            category = "capability"
        case .unsupported:
            code = "unsupported"
            category = "capability"
        default:
            code = "backendFailure"
            category = "platform"
        }
    } else {
        code = "backendFailure"
        category = "platform"
    }
    return [
        "message": error.localizedDescription,
        "code": code,
        "category": category,
        "retriable": false,
        "details": [
            "exception": String(describing: type(of: error)),
        ],
    ]
}

func downloadErrorMap(from error: Error) -> [String: Any] {
    if let drmError = error as? VesperPlayerDrmUnsupportedError {
        return [
            "message": drmError.localizedDescription,
            "code": "unsupported",
            "category": "capability",
            "retriable": false,
            "details": drmError.details.merging(
                ["exception": String(describing: type(of: error))]
            ) { current, _ in current },
        ]
    }
    return [
        "code": "backendFailure",
        "category": "platform",
        "retriable": false,
        "message": error.localizedDescription,
        "details": [
            "exception": String(describing: type(of: error)),
        ],
    ]
}

func asFlutterError(_ error: Error, code: String) -> FlutterError {
    FlutterError(
        code: code,
        message: error.localizedDescription,
        details: errorMap(from: error)
    )
}

func downloadOutputFormat(from raw: String?) -> VesperDownloadOutputFormat? {
    switch raw {
    case "mp4":
        return .mp4
    case "mkv":
        return .mkv
    case "original":
        return .original
    default:
        return nil
    }
}

func downloadStreamKind(from raw: String?) -> VesperDownloadStreamKind {
    switch raw {
    case "video":
        return .video
    case "audio":
        return .audio
    case "secondaryAudio":
        return .secondaryAudio
    case "subtitle":
        return .subtitle
    case "auxiliary":
        return .auxiliary
    default:
        return .combined
    }
}

func asDownloadFlutterError(_ error: Error, code: String) -> FlutterError {
    FlutterError(
        code: code,
        message: error.localizedDescription,
        details: downloadErrorMap(from: error)
    )
}
