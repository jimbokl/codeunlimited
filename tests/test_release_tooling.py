"""Expose release-tooling suites and validate the published experiment evidence."""

import datetime
import json
import pathlib
import re
import subprocess
import unittest
from decimal import Decimal, localcontext

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

        lower_raw = raw.lower()
        for forbidden in (
            "/users/",
            "/volumes/",
            "prompt",
            "response",
            "hostname",
            "claude-",
            "gpt-",
            "gemini-",
        ):
            self.assertNotIn(forbidden, lower_raw)
        for key in (
            "control_start_git_sha",
            "control_end_git_sha",
            "treatment_end_git_sha",
            "evidence_git_sha",
            "release_git_sha",
        ):
            self.assertRegex(evidence["provenance"][key], re.compile(r"^[0-9a-f]{40}$"))

        def commit_timestamp(sha: str) -> int:
            result = subprocess.run(
                ["git", "show", "-s", "--format=%ct", sha],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            )
            return int(result.stdout.strip())

        provenance = evidence["provenance"]
        self.assertEqual(
            evidence["windows"]["control"]["started_unix"],
            commit_timestamp(provenance["control_start_git_sha"]),
        )
        self.assertEqual(
            evidence["windows"]["control"]["finished_unix"],
            commit_timestamp(provenance["control_end_git_sha"]) + 1,
        )
        self.assertEqual(
            evidence["windows"]["treatment"]["finished_unix"],
            commit_timestamp(provenance["treatment_end_git_sha"]) + 1,
        )
        release_commit = subprocess.run(
            ["git", "rev-parse", "v1.8.0^{commit}"],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
        self.assertEqual(provenance["release_git_sha"], release_commit)
        subprocess.run(
            [
                "git",
                "merge-base",
                "--is-ancestor",
                provenance["evidence_git_sha"],
                provenance["release_git_sha"],
            ],
            cwd=root,
            check=True,
        )
        changed_after_treatment = subprocess.run(
            [
                "git",
                "diff",
                "--name-only",
                f'{provenance["treatment_end_git_sha"]}..{provenance["evidence_git_sha"]}',
            ],
            cwd=root,
            check=True,
            capture_output=True,
            text=True,
        ).stdout.splitlines()
        self.assertLessEqual(
            set(changed_after_treatment),
            {
                "CHANGELOG.md",
                "docs/ACCURACY.md",
                "docs/experiments/2026-09-04-v1.7-v1.8.json",
            },
            "treatment must end after the final production, tooling, and test commit",
        )

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
        self.assertAlmostEqual(
            comparison["observed_capacity_change_percent"],
            100.0
            * ((control / control_tasks) / (treatment / treatment_tasks) - 1.0),
        )
        independent = evidence["independent_arithmetic_check"]
        control_totals = comparison["control"]["totals"]
        treatment_totals = comparison["treatment"]["totals"]
        for key in (*categories, "input_tokens", "output_tokens", "total_tokens"):
            self.assertEqual(
                independent["observed_delta_per_task"][key],
                treatment_totals[key] / treatment_tasks
                - control_totals[key] / control_tasks,
            )
        with localcontext() as context:
            context.prec = 50
            control_decimal = Decimal(control) / Decimal(control_tasks)
            treatment_decimal = Decimal(treatment) / Decimal(treatment_tasks)
            decimal_delta = treatment_decimal - control_decimal
            self.assertEqual(
                Decimal(independent["observed_input_change_percent_high_precision"]),
                decimal_delta / control_decimal * Decimal(100),
            )
            self.assertEqual(
                Decimal(independent["observed_capacity_change_percent_high_precision"]),
                (control_decimal / treatment_decimal - Decimal(1)) * Decimal(100),
            )
        self.assertTrue(independent["matches_cli_json"])
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
