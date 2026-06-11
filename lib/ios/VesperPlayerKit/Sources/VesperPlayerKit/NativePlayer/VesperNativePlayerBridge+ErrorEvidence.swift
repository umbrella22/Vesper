import Foundation

struct ResolvedBridgeError {
    let code: VesperPlayerErrorCode
    let category: VesperPlayerErrorCategory
    let retriable: Bool
    let message: String
    let details: [String: String]
    let capabilityFailureCause: VesperIOSCapabilityFailureCause?

    init(
        code: VesperPlayerErrorCode? = nil,
        category: VesperPlayerErrorCategory,
        retriable: Bool,
        message: String,
        details: [String: String] = [:],
        capabilityFailureCause: VesperIOSCapabilityFailureCause? = nil
    ) {
        self.code = code ?? Self.defaultCode(for: category)
        self.category = category
        self.retriable = retriable
        self.message = message
        self.details = details
        self.capabilityFailureCause = capabilityFailureCause
    }

    func toPlayerError() -> VesperPlayerError {
        VesperPlayerError(
            message: message,
            code: code,
            category: category,
            retriable: retriable,
            details: details
        )
    }

    func enrichedWithDetails(_ additionalDetails: [String: String]) -> ResolvedBridgeError {
        guard !additionalDetails.isEmpty else {
            return self
        }
        var enrichedDetails = details
        enrichedDetails.merge(additionalDetails) { current, _ in current }
        return ResolvedBridgeError(
            code: code,
            category: category,
            retriable: retriable,
            message: message,
            details: enrichedDetails,
            capabilityFailureCause: capabilityFailureCause
        )
    }

    func enrichedWithHdrFailureEvidence(
        _ evidence: VesperNativeHdrFailureEvidence?
    ) -> ResolvedBridgeError {
        guard let runtimeEvidence = VesperNativeHdrRuntimeFailureEvidence(
            resolvedError: self,
            hdrEvidence: evidence
        ) else {
            return self
        }
        return ResolvedBridgeError(
            code: code,
            category: category,
            retriable: retriable,
            message: message,
            details: runtimeEvidence.details,
            capabilityFailureCause: capabilityFailureCause
        )
    }

    private static func defaultCode(for category: VesperPlayerErrorCategory) -> VesperPlayerErrorCode {
        switch category {
        case .input:
            return .invalidArgument
        case .source:
            return .invalidSource
        case .decode:
            return .decodeFailure
        case .audioOutput:
            return .audioOutputUnavailable
        case .capability:
            return .unsupported
        case .playback:
            return .invalidState
        case .network, .platform:
            return .backendFailure
        }
    }
}

enum VesperIOSCapabilityFailureCause: String {
    case hostNativeFrameUnsupported
    case filePermissionDenied
    case decoderNotFound
    case decoderTemporarilyUnavailable
    case fileFormatNotRecognized
}

struct VesperNativeHdrRuntimeFailureEvidence {
    let resolvedError: ResolvedBridgeError
    let hdrEvidence: VesperNativeHdrFailureEvidence

    init?(
        resolvedError: ResolvedBridgeError,
        hdrEvidence: VesperNativeHdrFailureEvidence?
    ) {
        guard let hdrEvidence,
            resolvedError.category == .decode || resolvedError.category == .capability
        else {
            return nil
        }
        self.resolvedError = resolvedError
        self.hdrEvidence = hdrEvidence
    }

    var details: [String: String] {
        var values = resolvedError.details
        values.merge(hdrEvidence.details) { current, _ in current }
        if values["capabilityFailureCause"] == nil,
           let capabilityFailureCause = resolvedError.capabilityFailureCause {
            values["capabilityFailureCause"] = capabilityFailureCause.rawValue
        }
        values["iosRuntimeEvidenceSource"] = "hostKitHdrRuntimeFailureEvidence"
        values["iosRuntimeFailureCategory"] = resolvedError.category.rawValue
        values["iosRuntimeFailureRetriable"] = String(resolvedError.retriable)
        values["iosRuntimeFailureCode"] = resolvedError.code.rawValue
        return values
    }
}

struct VesperNativeHdrFailureEvidence {
    let sourceUri: String
    let sourceProtocol: VesperPlayerSourceProtocol
    let hdrKind: VesperPlaybackCapabilityHdrKind
    let recommendedPlaybackPath: VesperRecommendedPlaybackPath
    let confidence: VesperPlaybackCapabilityConfidence
    let hdrMetadata: VesperPlaybackCapabilityHdrMetadata?
    let missingCapabilities: [String]
    let diagnostics: [String: String]

    init?(source: VesperPlayerSource, result: VesperPlaybackCapabilityProbeResult) {
        guard result.hdrKind != .none,
            result.hdrKind != .unknown,
            result.recommendedPlaybackPath == .systemPlayer
        else {
            return nil
        }
        sourceUri = source.uri
        sourceProtocol = source.protocol
        hdrKind = result.hdrKind
        recommendedPlaybackPath = result.recommendedPlaybackPath
        confidence = result.confidence
        hdrMetadata = result.hdrMetadata
        missingCapabilities = result.missingCapabilities
        diagnostics = result.diagnostics
    }

    var details: [String: String] {
        var values = [
            "likelyHdrCapabilityIssue": "true",
            "hdrKind": hdrKind.rawValue,
            "recommendedPlaybackPath": recommendedPlaybackPath.rawValue,
            "confidence": confidence.rawValue,
            "sourceProtocol": sourceProtocol.rawValue,
            "sourceUri": sourceUri,
        ]
        values.merge(hdrMetadata?.failureEvidenceDetails ?? [:]) { current, _ in current }
        if !missingCapabilities.isEmpty {
            values["missingCapabilities"] = missingCapabilities.joined(separator: ",")
        }
        values.merge(diagnosticFailureEvidenceDetails) { current, _ in current }
        return values
    }

    private var diagnosticFailureEvidenceDetails: [String: String] {
        var values: [String: String] = [:]
        for key in [
            "hdrKindSource",
            "assetProbe",
            "assetVideoMetadataHdrKind",
            "assetVideoTrackCount",
            "assetVideoCodec",
            "assetVideoWidth",
            "assetVideoHeight",
            "assetVideoFrameRate",
            "assetVideoEstimatedDataRate",
            "sessionProbe",
            "displayHdrProbeAvailable",
            "displayHdrSupported",
            "displayGamut",
            "avPlayerEligibleForHDRPlayback",
            "hdrKindSupportBasis",
            "displayFrameRateSupported",
            "displayMaximumFramesPerSecond",
            "displayNativeWidth",
            "displayNativeHeight",
            "requestedWidth",
            "requestedHeight",
            "requestedFrameRate",
        ] {
            if let value = diagnostics[key], !value.isEmpty {
                values[key] = value
            }
        }
        return values
    }
}
