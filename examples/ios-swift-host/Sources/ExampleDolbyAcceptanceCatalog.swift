import Foundation
import VesperPlayerKit

let EXAMPLE_DOLBY_ACCEPTANCE_WIDEVINE_LICENSE_URI =
    "https://widevine-dash.ezdrm.com/proxy?pX=E8A6EE"
let EXAMPLE_DOLBY_ACCEPTANCE_BASE_URL =
    "https://ott.dolby.com/OnDelKits/Dolby_Vision_Online_Delivery_Kit/v1/test_signals"

let EXAMPLE_FAIRPLAY_LICENSE_URI_ENV = "VESPER_IOS_FAIRPLAY_LICENSE_URI"
let EXAMPLE_FAIRPLAY_CERTIFICATE_URI_ENV = "VESPER_IOS_FAIRPLAY_CERTIFICATE_URI"
let EXAMPLE_FAIRPLAY_CERTIFICATE_BASE64_ENV = "VESPER_IOS_FAIRPLAY_CERTIFICATE_BASE64"
let EXAMPLE_FAIRPLAY_LICENSE_HEADERS_JSON_ENV = "VESPER_IOS_FAIRPLAY_LICENSE_HEADERS_JSON"
let EXAMPLE_FAIRPLAY_AUTHORIZATION_ENV = "VESPER_IOS_FAIRPLAY_AUTHORIZATION"

let exampleDolbyAcceptanceFpsValues = [25, 30, 60]

struct ExampleFairPlayLocalConfiguration: Equatable {
    let licenseUri: String
    let certificateUri: String?
    let certificateBase64: String?
    let licenseHeaders: [String: String]

    var licenseUriHost: String {
        URL(string: licenseUri)?.host ?? "unknown"
    }

    var certificateUriHost: String? {
        guard let certificateUri else { return nil }
        return URL(string: certificateUri)?.host
    }

    var headerCount: Int {
        licenseHeaders.count
    }

    var summary: String {
        var parts = [
            "FairPlay configured",
            "license host: \(licenseUriHost)",
            "header count: \(headerCount)",
        ]
        if let certificateUriHost {
            parts.append("certificate host: \(certificateUriHost)")
        } else {
            parts.append("certificate: base64")
        }
        return parts.joined(separator: " · ")
    }

    var drmConfiguration: VesperPlayerDrmConfiguration {
        VesperPlayerDrmConfiguration(
            keySystem: "fairPlay",
            licenseUri: licenseUri,
            licenseHeaders: licenseHeaders,
            fairPlayCertificateUri: certificateUri,
            fairPlayCertificateBase64: certificateBase64
        )
    }
}

func exampleFairPlayLocalConfiguration(
    environment: [String: String] = ProcessInfo.processInfo.environment
) -> ExampleFairPlayLocalConfiguration? {
    guard let licenseUri = environment[EXAMPLE_FAIRPLAY_LICENSE_URI_ENV]?.exampleTrimmedNonEmpty,
          let licenseURL = URL(string: licenseUri),
          licenseURL.scheme?.exampleTrimmedNonEmpty != nil
    else {
        return nil
    }

    let certificateBase64 = environment[EXAMPLE_FAIRPLAY_CERTIFICATE_BASE64_ENV]?.exampleTrimmedNonEmpty
    let certificateUri = environment[EXAMPLE_FAIRPLAY_CERTIFICATE_URI_ENV]?.exampleTrimmedNonEmpty
    let validCertificateUri = certificateUri.flatMap { uri -> String? in
        guard let url = URL(string: uri),
              url.scheme?.exampleTrimmedNonEmpty != nil
        else {
            return nil
        }
        return uri
    }
    guard certificateBase64 != nil || validCertificateUri != nil else {
        return nil
    }

    var headers = parseExampleFairPlayLicenseHeaders(
        environment[EXAMPLE_FAIRPLAY_LICENSE_HEADERS_JSON_ENV]
    )
    if let authorization = environment[EXAMPLE_FAIRPLAY_AUTHORIZATION_ENV]?.exampleTrimmedNonEmpty {
        headers["Authorization"] = authorization
    }

    return ExampleFairPlayLocalConfiguration(
        licenseUri: licenseUri,
        certificateUri: validCertificateUri,
        certificateBase64: certificateBase64,
        licenseHeaders: headers
    )
}

private func parseExampleFairPlayLicenseHeaders(_ rawValue: String?) -> [String: String] {
    guard let rawValue = rawValue?.exampleTrimmedNonEmpty,
          let data = rawValue.data(using: .utf8),
          let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
    else {
        return [:]
    }
    return json.reduce(into: [String: String]()) { values, entry in
        guard let key = entry.key.exampleTrimmedNonEmpty else {
            return
        }
        if let value = entry.value as? String,
           let trimmedValue = value.exampleTrimmedNonEmpty {
            values[key] = trimmedValue
        }
    }
}

enum ExampleDolbyAcceptanceProfile: String, CaseIterable, Identifiable {
    case p5
    case p81
    case p84

    var id: String { rawValue }

    var deliveryKitSegment: String {
        switch self {
        case .p5:
            return "P5"
        case .p81:
            return "P8_1"
        case .p84:
            return "P8_4"
        }
    }

    var title: String {
        switch self {
        case .p5:
            return "P5"
        case .p81:
            return "P8.1"
        case .p84:
            return "P8.4"
        }
    }

    var sampleIdSegment: String {
        switch self {
        case .p5:
            return "P5"
        case .p81:
            return "P81"
        case .p84:
            return "P84"
        }
    }

    var dolbyVisionProfile: Int {
        switch self {
        case .p5:
            return 5
        case .p81, .p84:
            return 8
        }
    }

    var profileFamily: String {
        switch self {
        case .p5:
            return "profile5"
        case .p81:
            return "profile8.1"
        case .p84:
            return "profile8.4"
        }
    }

    var fallbackTarget: String {
        switch self {
        case .p5:
            return "none"
        case .p81:
            return "hdr10"
        case .p84:
            return "hlg"
        }
    }

    var transferFunction: String {
        switch self {
        case .p84:
            return "ARIB_STD_B67_HLG"
        case .p5, .p81:
            return "SMPTE_ST_2084_PQ"
        }
    }
}

enum ExampleDolbyAcceptanceDrmKind: String, CaseIterable, Identifiable {
    case clear
    case widevinePending
    case fairPlay

    var id: String { rawValue }

    var title: String {
        switch self {
        case .clear:
            return "Clear"
        case .widevinePending:
            return "Widevine pending"
        case .fairPlay:
            return "FairPlay"
        }
    }

    var sampleIdSegment: String {
        switch self {
        case .clear:
            return "CLEAR"
        case .widevinePending:
            return "WIDEVINE-PENDING"
        case .fairPlay:
            return "FAIRPLAY"
        }
    }

    var metadataValue: String {
        switch self {
        case .clear:
            return "none"
        case .widevinePending:
            return "widevinePending"
        case .fairPlay:
            return "fairPlay"
        }
    }
}

struct ExampleDolbyAcceptancePreset: Identifiable, Equatable {
    let id: String
    let label: String
    let profile: ExampleDolbyAcceptanceProfile
    let fps: Int
    let sourceProtocol: VesperPlayerSourceProtocol
    let drmKind: ExampleDolbyAcceptanceDrmKind
    let source: VesperPlayerSource
    let expectedHdrKind: String
    let manualGate: String
    let notes: [String]
    let enabled: Bool

    var isDrm: Bool {
        drmKind != .clear
    }

    var isPlayable: Bool {
        enabled && sourceProtocol == .hls && (drmKind == .clear || drmKind == .fairPlay)
    }

    var protocolLabel: String {
        switch sourceProtocol {
        case .dash:
            return "DASH"
        case .hls:
            return "HLS"
        default:
            return sourceProtocol.rawValue
        }
    }

    func toHdrEvidencePreset() -> ExampleHdrEvidenceSamplePreset {
        ExampleHdrEvidenceSamplePreset(
            sampleId: id,
            label: label,
            expectedAxis: "display",
            sourceMetadata: [
                "sourceUri": source.uri,
                "sourceKind": "remote",
                "container": protocolLabel.lowercased(),
                "manifestKind": protocolLabel.lowercased(),
                "codec": "dolby-vision",
                "sampleMimeType": "video/dolby-vision",
                "width": NSNull(),
                "height": NSNull(),
                "frameRate": Double(fps),
                "bitDepth": 10,
                "hdrKind": expectedHdrKind,
                "colorPrimaries": "BT.2020",
                "transferFunction": profile.transferFunction,
                "yCbCrMatrix": "BT.2020_NCL",
                "drmKind": drmKind.metadataValue,
                "manualGate": manualGate,
                "controlPurpose": "dolbyVisionAcceptance",
                "dolbyVision": [
                    "profile": profile.dolbyVisionProfile,
                    "profileFamily": profile.profileFamily,
                    "baseLayer": "hevc-main10",
                    "fallbackTarget": profile.fallbackTarget,
                    "containerEvidence": "dolby-vision-online-delivery-kit",
                ],
                "metadataTool": [
                    "name": "Dolby Vision Online Delivery Kit",
                    "version": "public",
                    "command": "catalog-url",
                ],
                "notes": [
                    "Dolby Vision Online Delivery Kit public URL; media is not bundled.",
                ] + notes,
            ]
        )
    }
}

func exampleDolbyAcceptanceUrl(
    profile: ExampleDolbyAcceptanceProfile,
    fps: Int,
    protocol sourceProtocol: VesperPlayerSourceProtocol,
    drmKind: ExampleDolbyAcceptanceDrmKind
) -> String {
    let protocolFile: String
    switch sourceProtocol {
    case .dash:
        protocolFile = "dash.mpd"
    case .hls:
        protocolFile = "master.m3u8"
    default:
        preconditionFailure("DASH or HLS only")
    }

    let pathKind: String
    switch drmKind {
    case .clear:
        pathKind = "clear"
    case .widevinePending:
        pathKind = "cenc"
    case .fairPlay:
        pathKind = "cbcs"
    }
    return "\(EXAMPLE_DOLBY_ACCEPTANCE_BASE_URL)/\(pathKind)/" +
        "\(profile.deliveryKitSegment)_\(fps)/\(protocolFile)"
}

func buildExampleDolbyAcceptanceCatalog(
    fairPlayConfiguration: ExampleFairPlayLocalConfiguration? = exampleFairPlayLocalConfiguration()
) -> [ExampleDolbyAcceptancePreset] {
    var presets: [ExampleDolbyAcceptancePreset] = []
    for profile in ExampleDolbyAcceptanceProfile.allCases {
        for fps in exampleDolbyAcceptanceFpsValues {
            presets.append(
                buildExampleDolbyAcceptancePreset(
                    profile: profile,
                    fps: fps,
                    protocol: .hls,
                    drmKind: .clear
                )
            )
            presets.append(
                buildExampleDolbyAcceptancePreset(
                    profile: profile,
                    fps: fps,
                    protocol: .dash,
                    drmKind: .clear,
                    enabled: false
                )
            )
            presets.append(
                buildExampleDolbyAcceptancePreset(
                    profile: profile,
                    fps: fps,
                    protocol: .dash,
                    drmKind: .widevinePending,
                    enabled: false
                )
            )
            presets.append(
                buildExampleDolbyAcceptancePreset(
                    profile: profile,
                    fps: fps,
                    protocol: .hls,
                    drmKind: .fairPlay,
                    fairPlayConfiguration: fairPlayConfiguration,
                    enabled: fairPlayConfiguration != nil
                )
            )
        }
    }
    return presets
}

let exampleDolbyAcceptanceCatalog = buildExampleDolbyAcceptanceCatalog()

func exampleDolbyAcceptancePreset(id: String) -> ExampleDolbyAcceptancePreset? {
    exampleDolbyAcceptanceCatalog.first { $0.id == id }
}

func filterDolbyAcceptancePresets(
    _ presets: [ExampleDolbyAcceptancePreset],
    drmKind: ExampleDolbyAcceptanceDrmKind,
    profile: ExampleDolbyAcceptanceProfile?,
    fps: Int?
) -> [ExampleDolbyAcceptancePreset] {
    presets.filter { preset in
        preset.drmKind == drmKind &&
            (profile == nil || preset.profile == profile) &&
            (fps == nil || preset.fps == fps)
    }
}

func exampleDolbyAcceptanceHdrEvidencePresets() -> [ExampleHdrEvidenceSamplePreset] {
    exampleDolbyAcceptanceCatalog
        .filter(\.isPlayable)
        .map { $0.toHdrEvidencePreset() }
}

private func buildExampleDolbyAcceptancePreset(
    profile: ExampleDolbyAcceptanceProfile,
    fps: Int,
    protocol sourceProtocol: VesperPlayerSourceProtocol,
    drmKind: ExampleDolbyAcceptanceDrmKind,
    fairPlayConfiguration: ExampleFairPlayLocalConfiguration? = nil,
    enabled: Bool = true
) -> ExampleDolbyAcceptancePreset {
    let protocolSegment: String
    switch sourceProtocol {
    case .dash:
        protocolSegment = "DASH"
    case .hls:
        protocolSegment = "HLS"
    default:
        preconditionFailure("DASH or HLS only")
    }

    let id = "DOLBY-DV-\(profile.sampleIdSegment)-\(fps)-" +
        "\(protocolSegment)-\(drmKind.sampleIdSegment)"
    let label = "\(profile.title) \(fps)fps \(protocolSegment) \(drmKind.title)"
    let uri = exampleDolbyAcceptanceUrl(
        profile: profile,
        fps: fps,
        protocol: sourceProtocol,
        drmKind: drmKind
    )
    let source = VesperPlayerSource(
        uri: uri,
        label: label,
        kind: .remote,
        protocol: sourceProtocol,
        drmConfiguration: drmKind == .fairPlay ? fairPlayConfiguration?.drmConfiguration : nil
    )
    var notes: [String] = []
    if sourceProtocol == .dash {
        notes.append("DASH is not used for iOS Dolby direct validation; use HLS AVPlayer presets.")
    }
    if drmKind == .widevinePending {
        notes.append("Widevine is Android-only in this SDK round; preset is metadata-only on iOS.")
    }
    if drmKind == .fairPlay {
        notes.append(fairPlayConfiguration?.summary ?? "FairPlay config required.")
    }
    if fps == 60 {
        notes.append("Dolby 60fps signal exercises the high-frame-rate validation bucket.")
    }
    notes.append("MP4 zip assets remain manual local-file material and are not bundled.")

    return ExampleDolbyAcceptancePreset(
        id: id,
        label: label,
        profile: profile,
        fps: fps,
        sourceProtocol: sourceProtocol,
        drmKind: drmKind,
        source: source,
        expectedHdrKind: "dolbyVision",
        manualGate: "requiresDolbyVisionDisplay",
        notes: notes,
        enabled: enabled
    )
}

private extension String {
    var exampleTrimmedNonEmpty: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
