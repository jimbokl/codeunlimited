import json
import os
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

from scripts import benchmark_local


class BenchmarkStatisticsTests(unittest.TestCase):
    def test_nearest_rank_uses_the_observed_95th_percentile(self) -> None:
        self.assertEqual(benchmark_local.nearest_rank([5.0, 1.0, 4.0, 2.0, 3.0], 0.95), 5.0)
        self.assertEqual(benchmark_local.nearest_rank([2.0, 1.0], 0.50), 1.0)

    def test_summary_reports_median_p95_and_maximum_rss(self) -> None:
        summary = benchmark_local.summarize(
            [
                {"wall_seconds": 3.0, "max_rss_bytes": 100},
                {"wall_seconds": 1.0, "max_rss_bytes": None},
                {"wall_seconds": 2.0, "max_rss_bytes": 300},
            ]
        )
        self.assertEqual(summary["wall_seconds"], {"median": 2.0, "p95": 3.0})
        self.assertEqual(summary["max_rss_bytes"], 300)


class BenchmarkOutputTests(unittest.TestCase):
    def test_existing_output_is_preserved_without_force(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "result.json"
            path.write_text("keep\n", encoding="utf-8")

            with self.assertRaises(FileExistsError):
                benchmark_local.write_output(path, {"new": True}, force=False)

            self.assertEqual(path.read_text(encoding="utf-8"), "keep\n")

    def test_force_replaces_output_without_leaving_a_temporary_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            path = root / "result.json"
            path.write_text("old\n", encoding="utf-8")

            benchmark_local.write_output(path, {"new": True}, force=True)

            self.assertEqual(json.loads(path.read_text(encoding="utf-8")), {"new": True})
            self.assertEqual([entry.name for entry in root.iterdir()], ["result.json"])


class BenchmarkScenarioTests(unittest.TestCase):
    def test_scenario_redacts_projects_findings_and_token_values(self) -> None:
        payload = {
            "sources": {
                "codex": {"requests": 7, "prompt_tokens": 999_999, "output_tokens": 88}
            },
            "scan": {
                "files_discovered": 4,
                "files_opened": 2,
                "files_skipped_by_date": 1,
                "files_skipped_by_index": 1,
                "usage_records": 7,
            },
            "top_projects": [{"project": "SECRET_PROJECT", "total_tokens": 999_999}],
            "findings": [{"detail": "SECRET_PROMPT"}],
        }
        code = f"import json; print(json.dumps({payload!r}))"

        result = benchmark_local.run_scenario(
            "redaction",
            [sys.executable, "-c", code],
            env={},
            runs=1,
        )

        encoded = json.dumps(result, sort_keys=True)
        self.assertNotIn("SECRET_PROJECT", encoded)
        self.assertNotIn("SECRET_PROMPT", encoded)
        self.assertNotIn("999999", encoded)
        sample = result["samples"][0]
        self.assertEqual(sample["source_requests"], {"codex": 7})
        self.assertEqual(sample["scan"]["files_opened"], 2)

    def test_main_returns_nonzero_and_writes_results_when_a_scenario_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "failed.json"
            status = benchmark_local.main(
                [
                    "--binary",
                    sys.executable,
                    "--runs",
                    "1",
                    "--output",
                    str(output),
                ]
            )

            self.assertNotEqual(status, 0)
            result = json.loads(output.read_text(encoding="utf-8"))
            self.assertTrue(any(item["status"] == "failed" for item in result["scenarios"]))

    def test_main_runs_when_datetime_has_no_utc_alias(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = pathlib.Path(directory) / "python-310.json"
            had_utc = hasattr(benchmark_local.dt, "UTC")
            utc = getattr(benchmark_local.dt, "UTC", None)
            if had_utc:
                del benchmark_local.dt.UTC
            try:
                status = benchmark_local.main(
                    [
                        "--binary",
                        sys.executable,
                        "--runs",
                        "1",
                        "--output",
                        str(output),
                    ]
                )
            finally:
                if had_utc:
                    benchmark_local.dt.UTC = utc

            self.assertNotEqual(status, 0)
            self.assertTrue(output.is_file())

    @unittest.skipUnless(os.name == "posix", "executable bits are POSIX-only")
    def test_non_executable_binary_is_recorded_as_a_failed_sample(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = pathlib.Path(directory) / "not-executable"
            binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            binary.chmod(0o644)

            with mock.patch.object(benchmark_local.platform, "system", return_value="Other"):
                result = benchmark_local.run_scenario(
                    "spawn-error", [str(binary)], env={}, runs=1
                )

            self.assertEqual(result["status"], "failed")
            self.assertIsNone(result["samples"][0]["exit_code"])
            self.assertFalse(result["samples"][0]["json_valid"])


class BenchmarkProvenanceTests(unittest.TestCase):
    def test_corpus_metadata_contains_only_aggregate_counts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            claude = root / "claude"
            codex = root / "codex"
            (claude / "projects").mkdir(parents=True)
            (codex / "sessions").mkdir(parents=True)
            (claude / "projects" / "one.jsonl").write_bytes(b"1234")
            (codex / "sessions" / "two.jsonl").write_bytes(b"123456")
            (codex / "sessions" / "ignore.txt").write_text("secret", encoding="utf-8")

            with mock.patch.dict(
                os.environ,
                {"CLAUDE_HOME": str(claude), "CODEX_HOME": str(codex)},
            ):
                metadata = benchmark_local._corpus_metadata()

            self.assertEqual(metadata, {"jsonl_files": 2, "bytes": 10})
            self.assertNotIn(str(root), json.dumps(metadata))

    def test_payload_provenance_is_self_identifying(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            output = root / "result.json"
            empty_claude = root / "claude"
            empty_codex = root / "codex"
            empty_claude.mkdir()
            empty_codex.mkdir()
            with mock.patch.dict(
                os.environ,
                {"CLAUDE_HOME": str(empty_claude), "CODEX_HOME": str(empty_codex)},
            ):
                benchmark_local.main(
                    [
                        "--binary",
                        sys.executable,
                        "--runs",
                        "1",
                        "--output",
                        str(output),
                    ]
                )

            payload = json.loads(output.read_text(encoding="utf-8"))
            self.assertIn("codeunlimited_version", payload["provenance"])
            self.assertIn("git_sha", payload["provenance"])
            self.assertIn("total_memory_bytes", payload["platform"])
            self.assertEqual(payload["corpus"], {"jsonl_files": 0, "bytes": 0})


if __name__ == "__main__":
    unittest.main()
