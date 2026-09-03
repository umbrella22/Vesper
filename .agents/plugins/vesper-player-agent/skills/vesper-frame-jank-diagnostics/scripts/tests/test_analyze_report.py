from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


TESTS_DIR = Path(__file__).resolve().parent
SCRIPT = TESTS_DIR.parent / "analyze_report.py"
FIXTURES = TESTS_DIR / "fixtures"
GOLDEN = TESTS_DIR / "goldens" / "expected.json"
SPEC = importlib.util.spec_from_file_location("analyze_report", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
ANALYZER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ANALYZER)


class AnalyzeReportTests(unittest.TestCase):
    def test_fixtures_match_golden(self) -> None:
        expected = json.loads(GOLDEN.read_text(encoding="utf-8"))
        for name, golden in expected.items():
            if name in {"malformed", "oversized"}:
                continue
            with self.subTest(name=name):
                report = ANALYZER.load_report(FIXTURES / f"{name}.json")
                result = ANALYZER.validate_report(report)
                self.assertEqual(result["status"], golden["status"])
                self.assertEqual(result["diagnosis"], golden["diagnosis"])
                self.assertEqual(
                    result["sampleSufficient"], golden["sampleSufficient"]
                )
                self.assertEqual(
                    len(result["validation"]["unknownValues"]),
                    golden["unknownValues"],
                )
                self.assertEqual(
                    len(result["validation"]["sensitiveWarnings"]),
                    golden["sensitiveWarnings"],
                )

    def test_malformed_fixture_is_rejected(self) -> None:
        with self.assertRaises(ANALYZER.ReportInputError):
            ANALYZER.load_report(FIXTURES / "malformed.json")

    def test_oversized_fixture_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "oversized.json"
            path.write_bytes(b" " * (ANALYZER.MAX_REPORT_BYTES + 1))
            with self.assertRaises(ANALYZER.ReportInputError):
                ANALYZER.load_report(path)

    def test_sensitive_output_never_repeats_values(self) -> None:
        report = ANALYZER.load_report(FIXTURES / "sensitive.json")
        result = ANALYZER.validate_report(report)
        rendered = json.dumps(result) + ANALYZER.render_markdown(result)
        self.assertNotIn("fake-private-value", rendered)
        self.assertIn("value omitted", rendered)

    def test_baseline_comparison_checks_probe_compatibility(self) -> None:
        healthy = ANALYZER.load_report(FIXTURES / "healthy.json")
        overlay = ANALYZER.load_report(FIXTURES / "overlay-correlated.json")
        comparison = ANALYZER.compare_reports(overlay, healthy)
        self.assertTrue(comparison["compatible"])
        self.assertAlmostEqual(
            comparison["deltas"]["overlayActiveJankRatio"], 0.1
        )
        playback = ANALYZER.load_report(FIXTURES / "playback-pressure.json")
        self.assertFalse(ANALYZER.compare_reports(playback, healthy)["compatible"])

    def test_stall_count_alone_does_not_imply_playback_pressure(self) -> None:
        report = ANALYZER.load_report(FIXTURES / "healthy.json")
        report["playback"]["stallCount"] = 1

        result = ANALYZER.validate_report(report)

        self.assertEqual(result["status"], "valid")
        self.assertEqual(result["diagnosis"], "noSignificantPressure")

    def test_native_evidence_covers_unpublished_steady_stall_duration(self) -> None:
        report = ANALYZER.load_report(FIXTURES / "healthy.json")
        report["playback"]["stallCount"] = 1
        report["diagnosis"]["kind"] = "playbackPressure"
        report["diagnosis"]["evidenceCodes"] = ["native_playback_pressure"]

        result = ANALYZER.validate_report(report)

        self.assertEqual(result["status"], "valid")
        self.assertEqual(result["diagnosis"], "playbackPressure")

    def test_cli_json_exit_codes(self) -> None:
        valid = subprocess.run(
            [sys.executable, str(SCRIPT), str(FIXTURES / "healthy.json"),
             "--format", "json"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(valid.returncode, 0, valid.stderr)
        self.assertEqual(json.loads(valid.stdout)["status"], "valid")
        malformed = subprocess.run(
            [sys.executable, str(SCRIPT), str(FIXTURES / "malformed.json"),
             "--format", "json"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(malformed.returncode, 2)
        self.assertEqual(json.loads(malformed.stdout)["status"], "invalid")


if __name__ == "__main__":
    unittest.main()
