"""Exercise the offline packet benchmark through the public CLI."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "benchmark_packets.py"
DEFAULT_BINARY = ROOT / "target" / "debug" / (
    "codeunlimited.exe" if os.name == "nt" else "codeunlimited"
)


class PacketBenchmarkTests(unittest.TestCase):
    _completed: subprocess.CompletedProcess[str] | None = None

    @classmethod
    def report(cls) -> dict[str, object]:
        binary = pathlib.Path(os.environ.get("CODEUNLIMITED_BIN", DEFAULT_BINARY))
        if not binary.is_file():
            raise AssertionError(
                f"compiled CLI prerequisite is missing: {binary}; "
                "build it before running the Python suite"
            )
        if cls._completed is None:
            cls._completed = subprocess.run(
                [sys.executable, str(SCRIPT), "--binary", str(binary), "--json"],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
                timeout=60,
            )
        result = cls._completed
        assert result is not None
        if result.returncode != 0:
            raise AssertionError(
                f"benchmark exited {result.returncode}\nstdout:\n{result.stdout}"
                f"\nstderr:\n{result.stderr}"
            )
        return json.loads(result.stdout)

    def test_both_fixture_arms_accept_the_four_literal_tasks(self) -> None:
        report = self.report()
        arms = report["arms"]
        expected = ["unit-a", "unit-b", "unit-c", "unit-d"]
        self.assertEqual(arms["one_task_packets"]["accepted_task_ids"], expected)
        self.assertEqual(arms["four_task_packet"]["accepted_task_ids"], expected)

    def test_fixture_arms_have_equivalent_independently_checked_final_files(self) -> None:
        report = self.report()
        arms = report["arms"]
        expected_digest = "fa9402c38361da88f1b9042458c03575d18a0ad803ce0379a0fa79a828e73064"
        self.assertTrue(report["equivalent_final_files"])
        self.assertEqual(report["identical_final_tree_sha256"], expected_digest)
        self.assertEqual(
            arms["one_task_packets"]["final_tree_sha256"], expected_digest
        )
        self.assertEqual(arms["four_task_packet"]["final_tree_sha256"], expected_digest)

    def test_report_records_literal_process_counts_and_prompt_byte_totals(self) -> None:
        report = self.report()
        arms = report["arms"]
        self.assertEqual(arms["one_task_packets"]["worker_process_count"], 4)
        self.assertEqual(arms["four_task_packet"]["worker_process_count"], 1)
        self.assertEqual(
            arms["one_task_packets"]["process_count_basis"],
            "successful_deterministic_fixture_attempts",
        )
        self.assertEqual(
            arms["four_task_packet"]["process_count_basis"],
            "successful_deterministic_fixture_attempts",
        )
        self.assertGreater(arms["one_task_packets"]["prompt_bytes_total"], 0)
        self.assertGreater(arms["four_task_packet"]["prompt_bytes_total"], 0)

    def test_report_states_evidence_limits_and_contains_no_private_material(self) -> None:
        report = self.report()
        self.assertEqual(report["schema_version"], 1)
        self.assertEqual(report["evidence_scope"], "synthetic_offline")
        self.assertIsNone(report["real_token_savings_percent"])
        self.assertIn("real_token_totals", report)
        self.assertIsNone(report["real_token_totals"])
        self.assertIn("model_request_count", report)
        self.assertIsNone(report["model_request_count"])
        self.assertEqual(report["native_agent_comparison"], "not_run")
        self.assertEqual(report["provider_model_calls"], "none")
        self.assertEqual(
            report["prompt_byte_semantics"],
            "rendered run prompt bytes before each successful fixture step",
        )
        self.assertEqual(
            report["provenance"]["binary_source_attestation"],
            "not_available_for_caller_supplied_binary",
        )
        serialized = json.dumps(report, sort_keys=True).lower()
        for forbidden in (
            "/users/", "/volumes/",
            "alpha\\n", "bravo\\n", "charlie\\n", "delta\\n",
            "units/a.txt", "units/b.txt", "units/c.txt", "units/d.txt",
        ):
            self.assertNotIn(forbidden, serialized)

    def test_saved_evidence_has_reproducible_source_and_fixture_provenance(self) -> None:
        path = ROOT / "docs" / "experiments" / "2026-09-04-v2.2-packets.json"
        report = json.loads(path.read_text(encoding="utf-8"))
        provenance = report["provenance"]
        revision = provenance["source_revision"]
        self.assertRegex(revision, r"^[0-9a-f]{40}$")
        self.assertFalse(provenance["source_dirty"])

        for relative, digest_key in (
            ("scripts/benchmark_packets.py", "benchmark_script_sha256"),
            ("tests/fixtures/packet_driver.py", "fixture_sha256"),
        ):
            source = subprocess.run(
                ["git", "show", f"{revision}:{relative}"],
                cwd=ROOT,
                check=True,
                capture_output=True,
            ).stdout
            self.assertEqual(hashlib.sha256(source).hexdigest(), provenance[digest_key])

        serialized = json.dumps(report, sort_keys=True).lower()
        for forbidden in (
            "/users/", "/volumes/",
            "alpha\\n", "bravo\\n", "charlie\\n", "delta\\n",
            "units/a.txt", "units/b.txt", "units/c.txt", "units/d.txt",
        ):
            self.assertNotIn(forbidden, serialized)


if __name__ == "__main__":
    unittest.main()
