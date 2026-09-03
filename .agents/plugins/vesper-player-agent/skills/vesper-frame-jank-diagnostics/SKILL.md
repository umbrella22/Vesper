---
name: vesper-frame-jank-diagnostics
description: Use when validating, comparing, or interpreting Vesper Performance Diagnostics schema v1 reports; separating frame pressure from native playback pressure; reviewing Flutter FrameTiming, Android FrameMetrics, or iOS CADisplayLink probes; or planning a controlled overlay A/B capture without claiming causation.
---

# Vesper Frame Jank Diagnostics

## Load First

- `../../references/knowledge-map.md`
- `../../references/performance-diagnostics.md`
- `../../references/platform-hosts.md` when a platform probe is involved
- `../../references/validation-contract.md` when the task includes SDK or
  device acceptance
- The checkout's `docs/performance-diagnostics.md` and current implementation
  when a Vesper repository is available

## Analyze a Report

Use the bundled analyzer before interpreting report values:

```sh
python3 scripts/analyze_report.py REPORT --format markdown
python3 scripts/analyze_report.py REPORT --baseline BASELINE --format json
```

The analyzer rejects inputs larger than 4 MiB, malformed JSON, unsupported
schema versions, non-finite values, invalid nanosecond fields, incoherent
cohorts, and impossible count or percentile relationships. Unknown report
fields and raw enum strings remain visible as forward-compatible metadata.
Suspected credentials, URLs, account data, overlay text, or raw errors produce
redacted warnings; never repeat their values in analysis.

Treat validation, evidence sufficiency, and diagnosis as separate questions:

1. Confirm schema v1 structure and nanosecond wire units.
2. Confirm both steady cohorts contain at least 120 frames.
3. Compare only reports with compatible platform, probe, frame budget, content,
   device state, and refresh-rate conditions.
4. Read UI pressure, overlay correlation, and playback pressure independently.
5. State the probe's blind spots before giving an interpretation.
6. Describe correlation only. Do not identify the overlay, player, decoder,
   network, or media as the cause from this report alone.

## Controlled Overlay A/B Capture

- Wait for stable playback and non-empty overlay input for at most 10 seconds.
- Exclude the diagnostics panel itself from measured samples.
- Ignore a 5-second warm-up before the sequence.
- Run `off -> on -> off -> on`, 12 seconds per segment.
- Classify the first second after each switch as `transition`; classify the
  remaining measured frames as `steady`.
- Restore the complete prior overlay configuration on completion, cancellation,
  source replacement, navigation, or lifecycle interruption.
- Keep media, volume, device state, orientation, refresh rate, and content
  constant. A baseline from another probe or device is contextual only.

## Interpretation Rules

- `overlayCorrelatedUiPressure` means overlay-active steady frames correlate
  with more UI pressure under the v1 thresholds. It does not prove the overlay
  caused the pressure.
- `hostUiPressureUncorrelated` means UI pressure was observed without the
  required on/off delta.
- `playbackPressure` means native playback signals crossed a v1 threshold while
  the overlay correlation threshold did not.
- A nonzero stall count is context, not a threshold by itself. Because schema
  v1 does not publish steady-only stall duration, retain the sink's
  `native_playback_pressure` evidence when validating a stall-based diagnosis.
- `mixedPressure` means both signal families crossed their thresholds.
- `noSignificantPressure` means the fixed thresholds were not exceeded; it is
  not proof that every frame was smooth.
- `insufficientEvidence` is the only valid conclusion when either steady cohort
  has fewer than 120 frames.

Confidence is based on the smaller steady cohort: 120-299 is low, 300-599 is
medium, and at least 600 plus two overlay transitions is high. Unknown probe,
diagnosis, confidence, or severity strings must be reported as unknown values,
not coerced into a known case.

## Privacy Boundary

The official plugin performs no upload and must not receive media URLs, request
headers, cookies, account data, overlay text, or raw error messages. Reports may
still contain host-authored marker names or future extensions, so inspect and
redact before sharing. The analyzer warns but does not print suspected sensitive
values.

## Reporting

State the report status, platform, probe, frame budget, steady cohort counts,
jank ratio and p95, playback drop/buffering/stall summary, diagnosis,
confidence, dropped-event counts, unknown values, and comparison compatibility.
Name missing device or native-probe evidence explicitly. Keep implementation
findings separate from physical-device observations.

## Validation

Run the analyzer regression suite and skill validator after editing this skill:

```sh
python3 -m unittest discover \
  skills/vesper-frame-jank-diagnostics/scripts/tests -v
python3 /path/to/skill-creator/scripts/quick_validate.py \
  skills/vesper-frame-jank-diagnostics
```
