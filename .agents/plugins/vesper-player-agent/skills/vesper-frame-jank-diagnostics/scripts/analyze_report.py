#!/usr/bin/env python3
"""Validate and summarize Vesper Performance Diagnostics schema v1 reports."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import re
import sys
from typing import Any


MAX_REPORT_BYTES = 4 * 1024 * 1024
COHORT_NAMES = ("overlayInactive", "overlayActive", "transition", "excluded")
COHORT_FIELDS = {
    "sampleCount", "jankCount", "severeJankCount", "jankRatio",
    "severeJankRatio", "minLoadNs", "p50LoadNs", "p95LoadNs", "maxLoadNs",
}
TOP_LEVEL_FIELDS = {
    "schemaVersion", "runId", "sessionId", "platform", "probe", "durationNs",
    "frameBudgetNs", "cohorts", "playback", "diagnosis", "acceptedEvents",
    "droppedEvents", "rawEventsDropped", "diagnostics", "rawEvents",
}
PLAYBACK_FIELDS = {
    "activeDurationNs", "droppedVideoFrames", "bufferingCount",
    "bufferingDurationNs", "stallCount",
}
DIAGNOSIS_FIELDS = {"kind", "confidence", "evidenceCodes"}
DIAGNOSTIC_FIELDS = {"code", "severity", "message", "attributes"}
KNOWN_PROBES = {"flutterFrameTiming", "androidFrameMetrics", "iosDisplayLink"}
KNOWN_DIAGNOSES = {
    "insufficientEvidence", "noSignificantPressure",
    "overlayCorrelatedUiPressure", "hostUiPressureUncorrelated",
    "playbackPressure", "mixedPressure",
}
KNOWN_CONFIDENCE = {"low", "medium", "high"}
KNOWN_SEVERITIES = {"info", "warning", "error"}
SENSITIVE_KEY = re.compile(
    r"(?:authorization|cookie|password|secret|access[_-]?token|request[_-]?headers?"
    r"|media[_-]?url|video[_-]?url|account|danmaku(?:text|content)?"
    r"|comment(?:text|content)?|raw[_-]?error(?:message)?)",
    re.IGNORECASE,
)
SENSITIVE_VALUE = re.compile(
    r"(?:https?://|\bBearer\s+[A-Za-z0-9._~+/=-]+|(?:^|;\s*)[^=;\s]+=\S+;)",
    re.IGNORECASE,
)


class ReportInputError(ValueError):
    """Raised when a report cannot be read or parsed safely."""


def _reject_json_constant(value: str) -> None:
    raise ReportInputError(f"non-finite JSON number: {value}")


def load_report(path: Path) -> dict[str, Any]:
    try:
        size = path.stat().st_size
    except OSError as error:
        raise ReportInputError(f"cannot read report: {error}") from error
    if size > MAX_REPORT_BYTES:
        raise ReportInputError(
            f"report exceeds the {MAX_REPORT_BYTES}-byte input limit"
        )
    try:
        payload = path.read_bytes()
    except OSError as error:
        raise ReportInputError(f"cannot read report: {error}") from error
    if len(payload) > MAX_REPORT_BYTES:
        raise ReportInputError(
            f"report exceeds the {MAX_REPORT_BYTES}-byte input limit"
        )
    try:
        decoded = json.loads(
            payload.decode("utf-8"), parse_constant=_reject_json_constant
        )
    except (UnicodeDecodeError, json.JSONDecodeError, ReportInputError) as error:
        raise ReportInputError(f"invalid JSON report: {error}") from error
    if not isinstance(decoded, dict):
        raise ReportInputError("report root must be a JSON object")
    return decoded


def _path(parent: str, key: str) -> str:
    return f"{parent}.{key}" if parent else key


def _is_number(value: Any) -> bool:
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _nonnegative_int(
    obj: dict[str, Any], key: str, location: str, errors: list[str]
) -> int | None:
    value = obj.get(key)
    field = _path(location, key)
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        errors.append(f"{field} must be a nonnegative integer")
        return None
    return value


def _ratio(
    obj: dict[str, Any], key: str, location: str, errors: list[str]
) -> float | None:
    value = obj.get(key)
    field = _path(location, key)
    if not _is_number(value) or not math.isfinite(float(value)):
        errors.append(f"{field} must be a finite ratio")
        return None
    ratio = float(value)
    if ratio < 0 or ratio > 1:
        errors.append(f"{field} must be between 0 and 1")
        return None
    return ratio


def _required_string(
    obj: dict[str, Any], key: str, location: str, errors: list[str]
) -> str | None:
    value = obj.get(key)
    field = _path(location, key)
    if not isinstance(value, str) or not value:
        errors.append(f"{field} must be a non-empty string")
        return None
    return value


def _record_unknown_fields(
    obj: dict[str, Any], known: set[str], location: str, output: list[str]
) -> None:
    output.extend(_path(location, key) for key in obj if key not in known)


def _scan_finite(value: Any, location: str, errors: list[str]) -> None:
    if isinstance(value, float) and not math.isfinite(value):
        errors.append(f"{location or '$'} contains a non-finite number")
    elif isinstance(value, dict):
        for key, child in value.items():
            _scan_finite(child, _path(location, str(key)), errors)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _scan_finite(child, f"{location}[{index}]", errors)


def _scan_sensitive(value: Any, location: str, warnings: list[str]) -> None:
    if isinstance(value, dict):
        for key, child in value.items():
            child_path = _path(location, str(key))
            if SENSITIVE_KEY.search(str(key)):
                warnings.append(f"{child_path}: suspected sensitive key; value omitted")
            elif isinstance(child, str) and SENSITIVE_VALUE.search(child):
                warnings.append(f"{child_path}: suspected sensitive value; value omitted")
            _scan_sensitive(child, child_path, warnings)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            _scan_sensitive(child, f"{location}[{index}]", warnings)


def _unknown_value(
    path: str, value: str, known: set[str], output: list[dict[str, str]]
) -> None:
    if value not in known:
        output.append({
            "path": path,
            "rawValue": "<redacted>" if SENSITIVE_VALUE.search(value) else value,
        })


def _validate_cohort(
    name: str,
    value: Any,
    errors: list[str],
    unknown_fields: list[str],
) -> dict[str, Any] | None:
    location = f"cohorts.{name}"
    if not isinstance(value, dict):
        errors.append(f"{location} must be an object")
        return None
    _record_unknown_fields(value, COHORT_FIELDS, location, unknown_fields)
    sample_count = _nonnegative_int(value, "sampleCount", location, errors)
    jank_count = _nonnegative_int(value, "jankCount", location, errors)
    severe_count = _nonnegative_int(value, "severeJankCount", location, errors)
    jank_ratio = _ratio(value, "jankRatio", location, errors)
    severe_ratio = _ratio(value, "severeJankRatio", location, errors)
    loads = [
        _nonnegative_int(value, key, location, errors)
        for key in ("minLoadNs", "p50LoadNs", "p95LoadNs", "maxLoadNs")
    ]
    if sample_count is not None and jank_count is not None and jank_count > sample_count:
        errors.append(f"{location}.jankCount cannot exceed sampleCount")
    if jank_count is not None and severe_count is not None and severe_count > jank_count:
        errors.append(f"{location}.severeJankCount cannot exceed jankCount")
    if sample_count is not None and jank_count is not None and jank_ratio is not None:
        expected = jank_count / sample_count if sample_count else 0.0
        if not math.isclose(jank_ratio, expected, rel_tol=1e-6, abs_tol=1e-9):
            errors.append(f"{location}.jankRatio does not match its counts")
    if sample_count is not None and severe_count is not None and severe_ratio is not None:
        expected = severe_count / sample_count if sample_count else 0.0
        if not math.isclose(severe_ratio, expected, rel_tol=1e-6, abs_tol=1e-9):
            errors.append(f"{location}.severeJankRatio does not match its counts")
    if all(item is not None for item in loads):
        concrete = [int(item) for item in loads if item is not None]
        if concrete != sorted(concrete):
            errors.append(f"{location} load percentiles must be monotonic")
        if sample_count == 0 and any(concrete):
            errors.append(f"{location} with no samples must have zero load values")
    return value


def _expected_diagnosis(report: dict[str, Any]) -> str | None:
    cohorts = report.get("cohorts")
    playback = report.get("playback")
    diagnosis = report.get("diagnosis")
    budget = report.get("frameBudgetNs")
    if not isinstance(cohorts, dict) or not isinstance(playback, dict):
        return None
    inactive = cohorts.get("overlayInactive")
    active = cohorts.get("overlayActive")
    if not isinstance(inactive, dict) or not isinstance(active, dict):
        return None
    counts = (inactive.get("sampleCount"), active.get("sampleCount"))
    if not all(isinstance(item, int) and not isinstance(item, bool) for item in counts):
        return None
    if counts[0] < 120 or counts[1] < 120:
        return "insufficientEvidence"
    numeric_fields = (
        inactive.get("jankRatio"), active.get("jankRatio"),
        inactive.get("p95LoadNs"), active.get("p95LoadNs"), budget,
        playback.get("activeDurationNs"), playback.get("droppedVideoFrames"),
        playback.get("bufferingDurationNs"), playback.get("stallCount"),
    )
    if not all(_is_number(item) and math.isfinite(float(item)) for item in numeric_fields):
        return None
    if float(budget) <= 0:
        return None
    inactive_jank = float(inactive["jankRatio"])
    active_jank = float(active["jankRatio"])
    relative = math.inf if inactive_jank == 0 and active_jank > 0 else (
        1.0 if inactive_jank == 0 else active_jank / inactive_jank
    )
    correlated = (
        active_jank - inactive_jank >= 0.05 and relative >= 1.5
    ) or (
        int(active["p95LoadNs"]) - int(inactive["p95LoadNs"])
        >= int(budget) // 2
    )
    ui_pressure = (
        inactive_jank >= 0.05 or active_jank >= 0.05
        or int(inactive["p95LoadNs"]) > int(budget)
        or int(active["p95LoadNs"]) > int(budget)
    )
    active_minutes = float(playback["activeDurationNs"]) / 60_000_000_000
    dropped_threshold = max(3, math.ceil(active_minutes * 5))
    evidence_codes = diagnosis.get("evidenceCodes", []) if isinstance(diagnosis, dict) else []
    native_pressure_evidence = (
        isinstance(evidence_codes, list)
        and "native_playback_pressure" in evidence_codes
    )
    duration_pressure_possible = (
        int(playback["bufferingDurationNs"]) >= 500_000_000
        or int(playback["stallCount"]) > 0
    )
    playback_pressure = (
        int(playback["droppedVideoFrames"]) >= dropped_threshold
        or (native_pressure_evidence and duration_pressure_possible)
    )
    if playback_pressure and correlated:
        return "mixedPressure"
    if playback_pressure:
        return "playbackPressure"
    if correlated:
        return "overlayCorrelatedUiPressure"
    if ui_pressure:
        return "hostUiPressureUncorrelated"
    return "noSignificantPressure"


def validate_report(report: dict[str, Any]) -> dict[str, Any]:
    errors: list[str] = []
    warnings: list[str] = []
    unknown_fields: list[str] = []
    unknown_values: list[dict[str, str]] = []
    sensitive_warnings: list[str] = []
    _scan_finite(report, "", errors)
    _scan_sensitive(report, "", sensitive_warnings)
    _record_unknown_fields(report, TOP_LEVEL_FIELDS, "", unknown_fields)

    schema = report.get("schemaVersion")
    if schema != 1 or isinstance(schema, bool):
        errors.append("schemaVersion must be the integer 1")
    for key in ("runId", "sessionId", "platform", "probe"):
        _required_string(report, key, "", errors)
    for key in (
        "durationNs", "frameBudgetNs", "acceptedEvents", "droppedEvents",
        "rawEventsDropped",
    ):
        _nonnegative_int(report, key, "", errors)

    probe = report.get("probe")
    if isinstance(probe, str):
        _unknown_value("probe", probe, KNOWN_PROBES, unknown_values)

    cohorts = report.get("cohorts")
    validated_cohorts: dict[str, Any] = {}
    if not isinstance(cohorts, dict):
        errors.append("cohorts must be an object")
    else:
        for name in COHORT_NAMES:
            if name not in cohorts:
                errors.append(f"cohorts.{name} is required")
                continue
            candidate = _validate_cohort(name, cohorts[name], errors, unknown_fields)
            if candidate is not None:
                validated_cohorts[name] = candidate
        unknown_fields.extend(
            f"cohorts.{name}" for name in cohorts if name not in COHORT_NAMES
        )

    playback = report.get("playback")
    if not isinstance(playback, dict):
        errors.append("playback must be an object")
    else:
        _record_unknown_fields(playback, PLAYBACK_FIELDS, "playback", unknown_fields)
        for key in PLAYBACK_FIELDS:
            _nonnegative_int(playback, key, "playback", errors)

    diagnosis = report.get("diagnosis")
    diagnosis_kind: str | None = None
    confidence: str | None = None
    if not isinstance(diagnosis, dict):
        errors.append("diagnosis must be an object")
    else:
        _record_unknown_fields(diagnosis, DIAGNOSIS_FIELDS, "diagnosis", unknown_fields)
        diagnosis_kind = _required_string(diagnosis, "kind", "diagnosis", errors)
        confidence = _required_string(diagnosis, "confidence", "diagnosis", errors)
        evidence = diagnosis.get("evidenceCodes")
        if not isinstance(evidence, list) or not all(isinstance(item, str) for item in evidence):
            errors.append("diagnosis.evidenceCodes must be an array of strings")
        if diagnosis_kind is not None:
            _unknown_value("diagnosis.kind", diagnosis_kind, KNOWN_DIAGNOSES, unknown_values)
        if confidence is not None:
            _unknown_value("diagnosis.confidence", confidence, KNOWN_CONFIDENCE, unknown_values)

    diagnostics = report.get("diagnostics")
    if not isinstance(diagnostics, list):
        errors.append("diagnostics must be an array")
    else:
        for index, diagnostic in enumerate(diagnostics):
            location = f"diagnostics[{index}]"
            if not isinstance(diagnostic, dict):
                errors.append(f"{location} must be an object")
                continue
            _record_unknown_fields(diagnostic, DIAGNOSTIC_FIELDS, location, unknown_fields)
            _required_string(diagnostic, "code", location, errors)
            severity = _required_string(diagnostic, "severity", location, errors)
            _required_string(diagnostic, "message", location, errors)
            attributes = diagnostic.get("attributes")
            if not isinstance(attributes, dict) or not all(
                isinstance(key, str) and isinstance(item, str)
                for key, item in (attributes.items() if isinstance(attributes, dict) else [])
            ):
                errors.append(f"{location}.attributes must map strings to strings")
            if severity is not None:
                _unknown_value(
                    f"{location}.severity", severity, KNOWN_SEVERITIES, unknown_values
                )

    if not isinstance(report.get("rawEvents"), list):
        errors.append("rawEvents must be an array")

    inactive_samples = validated_cohorts.get("overlayInactive", {}).get("sampleCount", 0)
    active_samples = validated_cohorts.get("overlayActive", {}).get("sampleCount", 0)
    sample_sufficient = (
        isinstance(inactive_samples, int) and not isinstance(inactive_samples, bool)
        and isinstance(active_samples, int) and not isinstance(active_samples, bool)
        and inactive_samples >= 120 and active_samples >= 120
    )
    expected = _expected_diagnosis(report)
    if diagnosis_kind in KNOWN_DIAGNOSES and expected is not None and diagnosis_kind != expected:
        errors.append(
            f"diagnosis.kind is {diagnosis_kind} but schema v1 metrics imply {expected}"
        )
    minimum_samples = min(inactive_samples, active_samples)
    if confidence in KNOWN_CONFIDENCE and sample_sufficient:
        if minimum_samples < 300 and confidence != "low":
            errors.append("diagnosis.confidence must be low below 300 steady samples")
        elif 300 <= minimum_samples < 600 and confidence != "medium":
            errors.append("diagnosis.confidence must be medium for 300-599 steady samples")
        elif minimum_samples >= 600 and confidence == "low":
            errors.append("diagnosis.confidence cannot be low with at least 600 steady samples")

    warnings.extend(f"preserved unknown field: {item}" for item in unknown_fields)
    warnings.extend(
        f"preserved unknown raw value at {item['path']}: {item['rawValue']}"
        for item in unknown_values
    )
    if sensitive_warnings:
        warnings.append(
            "report contains suspected sensitive extensions or values; inspect and redact before sharing"
        )

    return {
        "status": "valid" if not errors else "invalid",
        "schemaVersion": schema,
        "platform": report.get("platform"),
        "probe": probe,
        "durationNs": report.get("durationNs"),
        "frameBudgetNs": report.get("frameBudgetNs"),
        "sampleSufficient": sample_sufficient,
        "cohorts": {
            name: {
                key: validated_cohorts.get(name, {}).get(key)
                for key in ("sampleCount", "jankRatio", "severeJankRatio", "p95LoadNs")
            }
            for name in COHORT_NAMES
        },
        "playback": {
            key: playback.get(key) if isinstance(playback, dict) else None
            for key in PLAYBACK_FIELDS
        },
        "diagnosis": diagnosis_kind,
        "confidence": confidence,
        "acceptedEvents": report.get("acceptedEvents"),
        "droppedEvents": report.get("droppedEvents"),
        "rawEventsDropped": report.get("rawEventsDropped"),
        "validation": {
            "errors": sorted(set(errors)),
            "warnings": sorted(set(warnings)),
            "unknownFields": sorted(set(unknown_fields)),
            "unknownValues": sorted(
                unknown_values, key=lambda item: (item["path"], item["rawValue"])
            ),
            "sensitiveWarnings": sorted(set(sensitive_warnings)),
        },
    }


def compare_reports(current: dict[str, Any], baseline: dict[str, Any]) -> dict[str, Any]:
    reasons = [
        f"{key} differs"
        for key in ("schemaVersion", "platform", "probe", "frameBudgetNs")
        if current.get(key) != baseline.get(key)
    ]

    def delta(path: tuple[str, ...]) -> float | int | None:
        left: Any = current
        right: Any = baseline
        for key in path:
            if not isinstance(left, dict) or not isinstance(right, dict):
                return None
            left, right = left.get(key), right.get(key)
        if not _is_number(left) or not _is_number(right):
            return None
        return left - right

    return {
        "compatible": not reasons,
        "compatibilityWarnings": reasons,
        "deltas": {
            "overlayInactiveJankRatio": delta(("cohorts", "overlayInactive", "jankRatio")),
            "overlayActiveJankRatio": delta(("cohorts", "overlayActive", "jankRatio")),
            "overlayInactiveP95LoadNs": delta(("cohorts", "overlayInactive", "p95LoadNs")),
            "overlayActiveP95LoadNs": delta(("cohorts", "overlayActive", "p95LoadNs")),
            "droppedVideoFrames": delta(("playback", "droppedVideoFrames")),
            "bufferingDurationNs": delta(("playback", "bufferingDurationNs")),
            "stallCount": delta(("playback", "stallCount")),
        },
    }


def _format_value(value: Any) -> str:
    if value is None:
        return "n/a"
    return f"{value:.6f}" if isinstance(value, float) else str(value)


def render_markdown(analysis: dict[str, Any]) -> str:
    validation = analysis["validation"]
    lines = [
        "# Vesper Performance Diagnostics Analysis", "",
        f"- Status: `{analysis['status']}`",
        f"- Platform / probe: `{analysis.get('platform')}` / `{analysis.get('probe')}`",
        f"- Frame budget: `{_format_value(analysis.get('frameBudgetNs'))} ns`",
        f"- Sample sufficiency: `{'sufficient' if analysis.get('sampleSufficient') else 'insufficient'}`",
        f"- Diagnosis / confidence: `{analysis.get('diagnosis')}` / `{analysis.get('confidence')}`",
        "", "## Frame Cohorts", "",
        "| Cohort | Samples | Jank ratio | Severe ratio | p95 (ns) |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    for name in COHORT_NAMES:
        cohort = analysis["cohorts"][name]
        lines.append(
            f"| {name} | {_format_value(cohort.get('sampleCount'))} | "
            f"{_format_value(cohort.get('jankRatio'))} | "
            f"{_format_value(cohort.get('severeJankRatio'))} | "
            f"{_format_value(cohort.get('p95LoadNs'))} |"
        )
    playback = analysis["playback"]
    lines.extend([
        "", "## Playback Pressure", "",
        f"- Active duration: `{_format_value(playback.get('activeDurationNs'))} ns`",
        f"- Dropped video frames: `{_format_value(playback.get('droppedVideoFrames'))}`",
        f"- Buffering: `{_format_value(playback.get('bufferingCount'))}` events / `{_format_value(playback.get('bufferingDurationNs'))} ns`",
        f"- Stalls: `{_format_value(playback.get('stallCount'))}`",
        f"- Accepted / dropped / raw dropped: `{_format_value(analysis.get('acceptedEvents'))}` / `{_format_value(analysis.get('droppedEvents'))}` / `{_format_value(analysis.get('rawEventsDropped'))}`",
    ])
    comparison = analysis.get("comparison")
    if comparison is not None:
        lines.extend(["", "## Baseline Comparison", "",
                      f"- Compatible: `{'yes' if comparison['compatible'] else 'no'}`"])
        lines.extend(
            f"- Compatibility warning: {item}"
            for item in comparison["compatibilityWarnings"]
        )
        lines.extend(
            f"- {key}: `{_format_value(value)}`"
            for key, value in comparison["deltas"].items()
        )
    if validation["errors"]:
        lines.extend(["", "## Validation Errors", ""])
        lines.extend(f"- {item}" for item in validation["errors"])
    if validation["warnings"]:
        lines.extend(["", "## Warnings", ""])
        lines.extend(f"- {item}" for item in validation["warnings"])
    if validation["sensitiveWarnings"]:
        lines.extend(["", "Sensitive values were omitted from this analysis."])
    lines.extend([
        "",
        "> The diagnosis describes correlation under schema v1 thresholds; it does not establish causation.",
        "",
    ])
    return "\n".join(lines)


def _argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate and summarize a Vesper performance report."
    )
    parser.add_argument("report", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--format", choices=("markdown", "json"), default="markdown")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _argument_parser().parse_args(argv)
    try:
        report = load_report(args.report)
        analysis = validate_report(report)
        if args.baseline is not None:
            baseline = load_report(args.baseline)
            baseline_analysis = validate_report(baseline)
            analysis["baselineValidation"] = baseline_analysis["validation"]
            analysis["comparison"] = compare_reports(report, baseline)
            if baseline_analysis["status"] != "valid":
                analysis["status"] = "invalid"
    except ReportInputError as error:
        failure = {"status": "invalid", "inputError": str(error)}
        if args.format == "json":
            print(json.dumps(failure, indent=2, sort_keys=True))
        else:
            print(
                "# Vesper Performance Diagnostics Analysis\n\n"
                f"- Status: `invalid`\n- Input error: {error}\n"
            )
        return 2

    if args.format == "json":
        print(json.dumps(analysis, indent=2, sort_keys=True))
    else:
        print(render_markdown(analysis), end="")
    return 0 if analysis["status"] == "valid" else 2


if __name__ == "__main__":
    sys.exit(main())
