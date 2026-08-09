import Foundation

/// Structured rejection for an explicit fixed-video-track command.
public struct VesperFixedTrackSelectionError: Error, LocalizedError {
    public let code: String
    public let trackId: String?
    public let expectedCatalogRevision: Int64?
    public let actualCatalogRevision: Int64?
    public let message: String
    public let details: [String: String]

    public init(
        code: String,
        trackId: String?,
        expectedCatalogRevision: Int64?,
        actualCatalogRevision: Int64?,
        message: String,
        details: [String: String] = [:]
    ) {
        self.code = code
        self.trackId = trackId
        self.expectedCatalogRevision = expectedCatalogRevision
        self.actualCatalogRevision = actualCatalogRevision
        self.message = message
        self.details = details
    }

    public var errorDescription: String? {
        message
    }

    var playerErrorDetails: [String: String] {
        var output = details
        output["domain"] = "fixedTrack"
        output["code"] = code
        if let trackId {
            output["trackId"] = trackId
        }
        if let expectedCatalogRevision {
            output["expectedCatalogRevision"] = String(expectedCatalogRevision)
        }
        if let actualCatalogRevision {
            output["actualCatalogRevision"] = String(actualCatalogRevision)
        }
        output["message"] = message
        return output
    }
}
