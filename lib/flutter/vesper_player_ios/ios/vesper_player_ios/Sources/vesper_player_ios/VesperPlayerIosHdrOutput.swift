import VesperPlayerKit

func flutterHdrOutputMap(_ output: VesperHdrOutputSnapshot?, playerId: String) -> [String: Any] {
    guard let output else {
        return ["state": "unknown", "playerId": playerId, "reason": "outputObservationUnavailable"]
    }
    return [
        "state": output.state.rawValue,
        "format": output.format.rawValue,
        "playerId": playerId,
        "sourceRevision": output.sourceRevision,
        "outputGeneration": output.outputGeneration,
        "effectiveVideoTrackId": flutterValue(output.effectiveVideoTrackId),
        "catalogRevision": flutterValue(output.catalogRevision),
        "displayId": flutterValue(output.displayId),
        "evidence": flutterValue(output.evidence),
        "reason": flutterValue(output.reason),
    ]
}
