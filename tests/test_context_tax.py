import contextlib
import io
import json
import pathlib
import tempfile
import unittest

from scripts import bench_context


def assistant_record(session: str, message: str, prompt: int, timestamp: str) -> str:
    return json.dumps(
        {
            "type": "assistant",
            "sessionId": session,
            "timestamp": timestamp,
            "message": {
                "id": message,
                "model": "private-model",
                "usage": {
                    "input_tokens": prompt,
                    "cache_read_input_tokens": 0,
                    "cache_creation_input_tokens": 0,
                },
            },
        }
    )


class ContextTaxTests(unittest.TestCase):
    def write_log(self, root: pathlib.Path, project: str, lines: list[str]) -> None:
        folder = root / project
        folder.mkdir(parents=True, exist_ok=True)
        (folder / "session.jsonl").write_text("\n".join(lines) + "\n", encoding="utf-8")

    def test_analysis_keeps_positive_and_negative_modeled_sessions(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            self.write_log(
                root,
                "project-a",
                [
                    assistant_record("positive", "p1", 100, "2026-01-01T00:00:01Z"),
                    assistant_record("positive", "p2", 100, "2026-01-01T00:00:02Z"),
                    assistant_record("positive", "p3", 400, "2026-01-01T00:00:03Z"),
                    assistant_record("negative", "n1", 200, "2026-01-01T00:00:01Z"),
                    assistant_record("negative", "n2", 200, "2026-01-01T00:00:02Z"),
                    assistant_record("negative", "n3", 50, "2026-01-01T00:00:03Z"),
                ],
            )

            loaded = bench_context.load_sessions(root)
            result = bench_context.analyze(
                loaded.sessions,
                min_turns=2,
                early_turns=2,
                malformed_candidates=loaded.malformed_candidates,
            )

        self.assertEqual(result["sessions_included"], 2)
        self.assertEqual(result["sessions_positive_modeled_difference"], 1)
        self.assertEqual(result["sessions_negative_modeled_difference"], 1)
        self.assertEqual(result["actual_prompt_tokens"], 1_050)
        self.assertEqual(result["modeled_bounded_prompt_tokens"], 900.0)
        self.assertEqual(result["modeled_difference_tokens"], 150.0)

    def test_threshold_is_strict_and_message_ids_are_scoped_to_session(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            duplicate = assistant_record("one", "shared", 100, "2026-01-01T00:00:01Z")
            self.write_log(
                root,
                "project-a",
                [
                    duplicate,
                    duplicate,
                    assistant_record("one", "one-2", 100, "2026-01-01T00:00:02Z"),
                    assistant_record("two", "shared", 200, "2026-01-01T00:00:01Z"),
                    assistant_record("two", "two-2", 200, "2026-01-01T00:00:02Z"),
                ],
            )
            loaded = bench_context.load_sessions(root)

        self.assertEqual(sorted(len(turns) for turns in loaded.sessions.values()), [2, 2])
        self.assertEqual(
            bench_context.analyze(loaded.sessions, min_turns=2, early_turns=1)[
                "sessions_included"
            ],
            0,
        )
        self.assertEqual(
            bench_context.analyze(loaded.sessions, min_turns=1, early_turns=1)[
                "sessions_included"
            ],
            2,
        )

    def test_malformed_candidate_is_counted_and_output_is_private(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            self.write_log(
                root,
                "secret-project-name",
                [
                    '{"type":"assistant","usage": broken',
                    assistant_record("secret-session", "m1", 10, "2026-01-01T00:00:01Z"),
                    assistant_record("secret-session", "m2", 20, "2026-01-01T00:00:02Z"),
                ],
            )
            loaded = bench_context.load_sessions(root)
            result = bench_context.analyze(
                loaded.sessions,
                min_turns=1,
                early_turns=1,
                malformed_candidates=loaded.malformed_candidates,
            )

        self.assertEqual(loaded.malformed_candidates, 1)
        self.assertFalse(result["complete_accounting"])
        serialized = json.dumps(result, sort_keys=True)
        self.assertNotIn("secret-project-name", serialized)
        self.assertNotIn("secret-session", serialized)
        self.assertNotIn("private-model", serialized)

    def test_non_object_json_candidate_is_counted_as_malformed(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            self.write_log(root, "project-a", ['["assistant", "usage"]'])

            loaded = bench_context.load_sessions(root)

        self.assertEqual(loaded.malformed_candidates, 1)
        self.assertEqual(loaded.recognized_records, 0)
        self.assertEqual(loaded.sessions, {})

    def test_missing_root_fails_instead_of_looking_like_zero_usage(self) -> None:
        missing = pathlib.Path(tempfile.gettempdir()) / "codeunlimited-missing-context-root"
        with self.assertRaises(FileNotFoundError):
            bench_context.load_sessions(missing)

        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = bench_context.main(["--root", str(missing)])
        self.assertEqual(status, 2)
        self.assertIn("does not exist", stderr.getvalue())
        self.assertNotIn(str(missing), stderr.getvalue())

    def test_invalid_utf8_fails_instead_of_silently_dropping_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            folder = root / "private-project"
            folder.mkdir()
            log = folder / "session.jsonl"
            log.write_bytes(
                assistant_record("private-session", "m1", 10, "2026-01-01T00:00:01Z").encode()
                + b"\n\xff\n"
            )

            with self.assertRaises(UnicodeDecodeError):
                bench_context.load_sessions(root)

            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                status = bench_context.main(["--root", str(root)])

        self.assertEqual(status, 2)
        self.assertIn("benchmark failed", stderr.getvalue())
        self.assertNotIn("private-project", stderr.getvalue())
        self.assertNotIn("private-session", stderr.getvalue())

    def test_default_root_is_cross_platform(self) -> None:
        self.assertEqual(
            bench_context.default_root(), pathlib.Path.home() / ".claude" / "projects"
        )

    def test_json_schema_distinguishes_observed_from_modeled(self) -> None:
        result = bench_context.analyze({}, min_turns=30, early_turns=5)
        self.assertEqual(result["schema_version"], 1)
        self.assertEqual(result["actual_prompt_tokens"], 0)
        self.assertIsNone(result["aggregate_multiplier"])
        self.assertIn("modeled_bounded_prompt_tokens", result)
        self.assertNotIn("tokens_burned", result)


if __name__ == "__main__":
    unittest.main()
