import Foundation

public struct VesperPlayerDrmUnsupportedError: LocalizedError {
    public let route: String
    public let keySystem: String
    public let reason: String
    private let message: String?
    private let additionalDetails: [String: String]

    public init(
        route: String,
        keySystem: String,
        reason: String,
        message: String? = nil,
        details: [String: String] = [:]
    ) {
        self.route = route
        self.keySystem = keySystem
        self.reason = reason
        self.message = message
        additionalDetails = details
    }

    public var errorDescription: String? {
        message ?? "DRM is not supported on the \(route) playback route."
    }

    public var details: [String: String] {
        var values = [
            "reason": reason,
            "route": route,
            "keySystem": keySystem,
        ]
        values.merge(additionalDetails) { current, _ in current }
        return values
    }
}

public struct VesperPlayerDrmRuntimeError: LocalizedError {
    public let reason: String
    public let route: String
    public let keySystem: String
    public let retriable: Bool
    private let message: String
    private let additionalDetails: [String: String]

    public init(
        reason: String,
        route: String,
        keySystem: String,
        message: String,
        retriable: Bool = false,
        details: [String: String] = [:]
    ) {
        self.reason = reason
        self.route = route
        self.keySystem = keySystem
        self.retriable = retriable
        self.message = message
        additionalDetails = details
    }

    public var errorDescription: String? {
        message
    }

    public var details: [String: String] {
        var values = [
            "reason": reason,
            "route": route,
            "keySystem": keySystem,
            "errorMessage": message,
        ]
        values.merge(additionalDetails) { current, _ in current }
        return values
    }
}

func vesperDrmPhase0Failure(
    for source: VesperPlayerSource,
    sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
    nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration
) -> VesperPlayerDrmUnsupportedError? {
    guard let drmConfiguration = source.drmConfiguration else {
        return nil
    }

    if let route = drmUnsupportedRoute(
        for: source,
        sourceNormalizerConfiguration: sourceNormalizerConfiguration,
        nativeFramePipelineConfiguration: nativeFramePipelineConfiguration
    ) {
        return VesperPlayerDrmUnsupportedError(
            route: route,
            keySystem: drmConfiguration.keySystem,
            reason: "drmUnsupportedRoute"
        )
    }

    guard source.protocol == .hls else {
        return VesperPlayerDrmUnsupportedError(
            route: "direct",
            keySystem: drmConfiguration.keySystem,
            reason: "drmUnsupportedRoute",
            message: "FairPlay DRM requires AVPlayer direct HLS playback.",
            details: ["sourceProtocol": source.protocol.rawValue]
        )
    }

    guard drmConfiguration.keySystem.caseInsensitiveCompare("fairPlay") == .orderedSame else {
        return VesperPlayerDrmUnsupportedError(
            route: "direct",
            keySystem: drmConfiguration.keySystem,
            reason: "drmUnsupportedKeySystem"
        )
    }

    guard let licenseUri = drmConfiguration.licenseUri.vesperTrimmedNonEmpty,
          let licenseURL = URL(string: licenseUri),
          licenseURL.scheme?.vesperTrimmedNonEmpty != nil
    else {
        return VesperPlayerDrmUnsupportedError(
            route: "direct",
            keySystem: drmConfiguration.keySystem,
            reason: "fairPlayLicenseUriInvalid",
            message: "FairPlay license URI must be a non-empty absolute URL."
        )
    }

    guard drmConfiguration.fairPlayCertificateBase64?.vesperTrimmedNonEmpty != nil ||
        drmConfiguration.fairPlayCertificateUri?.vesperTrimmedNonEmpty != nil
    else {
        return VesperPlayerDrmUnsupportedError(
            route: "direct",
            keySystem: drmConfiguration.keySystem,
            reason: "fairPlayCertificateMissing",
            message: "FairPlay requires a certificate URI or base64 certificate data.",
            details: fairPlayUriHostDetails(key: "licenseUriHost", url: licenseURL)
        )
    }

#if targetEnvironment(simulator)
    return VesperPlayerDrmUnsupportedError(
        route: "direct",
        keySystem: drmConfiguration.keySystem,
        reason: "fairPlaySimulatorUnsupported",
        message: "FairPlay Streaming content key sessions are not available on iOS Simulator.",
        details: fairPlayUriHostDetails(key: "licenseUriHost", url: licenseURL)
    )
#else
    return nil
#endif
}

private func drmUnsupportedRoute(
    for source: VesperPlayerSource,
    sourceNormalizerConfiguration: VesperSourceNormalizerConfiguration,
    nativeFramePipelineConfiguration: VesperNativeFramePipelineConfiguration
) -> String? {
    if source.protocol == .dash {
        return "dash"
    }
    if sourceNormalizerConfiguration.mode == .requireNormalized {
        return "sourceNormalizer"
    }
    if nativeFramePipelineConfiguration.mode == .requireNativeFrame {
        return "nativeFrame"
    }
    return nil
}

func drmUnsupportedRouteMessage(_ route: String) -> String {
    "DRM is not supported on the \(route) playback route."
}

func fairPlayUriHostDetails(key: String, url: URL?) -> [String: String] {
    guard let host = url?.host?.vesperTrimmedNonEmpty else {
        return [:]
    }
    return [key: host]
}
