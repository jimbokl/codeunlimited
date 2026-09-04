"""Expose release-tooling suites and validate the published experiment evidence."""

import datetime
import json
import pathlib
import re
import unittest

from scripts.test_benchmark_local import (
    BenchmarkOutputTests,
    BenchmarkProvenanceTests,
    BenchmarkScenarioTests,
    BenchmarkStatisticsTests,
)
from scripts.test_check_release import ReleaseCheckerTests


class ExperimentEvidenceTests(unittest.TestCase):
    def test_published_evidence_is_private_and_arithmetically_reproducible(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[1]
        path = root / "docs" / "experiments" / "2026-09-04-v1.7-v1.8.json"
        raw = path.read_text(encoding="utf-8")
        evidence = json.loads(raw)

        for forbidden in ("/Users/", "/Volumes/", '"prompt"', '"response"', '"hostname"'):
            self.assertNotIn(forbidden, raw)
        for key in ("control_start_git_sha", "control_end_git_sha", "treatment_end_git_sha"):
            self.assertRegex(evidence["provenance"][key], re.compile(r"^[0-9a-f]{40}$"))

        comparison = evidence["comparison_output"]
        categories = (
            "uncached_input_tokens",
            "cache_read_input_tokens",
            "cache_write_5m_input_tokens",
            "cache_write_1h_input_tokens",
        )
        for arm in ("control", "treatment"):
            record = evidence["record_outputs"][arm]
            self.assertEqual(record, comparison[arm])
            totals = record["totals"]
            self.assertEqual(totals["input_tokens"], sum(totals[key] for key in categories))
            self.assertEqual(
                totals["total_tokens"], totals["input_tokens"] + totals["output_tokens"]
            )
            window = evidence["windows"][arm]
            for field, unix_field in (
                ("from_rfc3339", "started_unix"),
                ("to_rfc3339", "finished_unix"),
            ):
                parsed = datetime.datetime.fromisoformat(window[field].replace("Z", "+00:00"))
                self.assertEqual(parsed.microsecond, 0)
                self.assertEqual(int(parsed.timestamp()), window[unix_field])

        control = comparison["control"]["totals"]["input_tokens"]
        treatment = comparison["treatment"]["totals"]["input_tokens"]
        control_tasks = comparison["control"]["completed_tasks"]
        treatment_tasks = comparison["treatment"]["completed_tasks"]
        expected_delta = treatment / treatment_tasks - control / control_tasks
        self.assertEqual(comparison["observed_input_delta_per_task"], expected_delta)
        self.assertAlmostEqual(
            comparison["observed_input_change_percent"],
            100.0 * expected_delta / (control / control_tasks),
        )
        self.assertEqual(comparison["confidence"], "low")
        self.assertEqual(comparison["causality"], "observational")


__all__ = [
    "BenchmarkOutputTests",
    "BenchmarkProvenanceTests",
    "BenchmarkScenarioTests",
    "BenchmarkStatisticsTests",
    "ExperimentEvidenceTests",
    "ReleaseCheckerTests",
]
