"""Compare the shipped CLI's model against the independent Python reference."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

from scripts import bench_context


ROOT = pathlib.Path(__file__).resolve().parents[1]


class VerdictReferenceTests(unittest.TestCase):
    def test_growing_flat_shrinking_and_mixed_histories_match(self):
        binary = pathlib.Path(os.environ.get("CODEUNLIMITED_BIN", ROOT / "target/debug/codeunlimited")).resolve()
        growing = [20_000 + 5_000 * i for i in range(40)]
        flat = [50_000] * 40
        shrinking = [100_000] * 5 + [10_000] * 35
        for sessions in ([growing], [flat], [shrinking], [growing, flat, shrinking]):
            with self.subTest(sessions=len(sessions), first=sessions[0][0]), tempfile.TemporaryDirectory() as temp:
                root = pathlib.Path(temp)
                folder = root / "claude/projects/p"
                folder.mkdir(parents=True)
                for s, values in enumerate(sessions):
                    rows = [json.dumps({
                        "type": "assistant", "sessionId": str(s),
                        "timestamp": f"2026-01-01T00:00:{i:02}Z",
                        "message": {"id": f"{s}-{i}", "model": "fixture", "usage": {"input_tokens": n, "output_tokens": 1}},
                    }) for i, n in enumerate(values)]
                    # Make file order differ from timestamp order.
                    (folder / f"{s}.jsonl").write_text("\n".join(reversed(rows)) + "\n", encoding="utf-8")
                loaded = bench_context.load_sessions(root / "claude/projects")
                reference = bench_context.analyze(loaded.sessions, min_turns=30, early_turns=5)
                env = dict(os.environ, CLAUDE_HOME=str(root / "claude"), CODEX_HOME=str(root / "codex"), CODEUNLIMITED_HOME=str(root / "state"))
                result = subprocess.run([str(binary), "verdict", "--json"], env=env, capture_output=True, text=True, timeout=30, check=True)
                actual = json.loads(result.stdout)
                self.assertTrue(actual["complete_accounting"])
                self.assertEqual(actual["observed_prompt_tokens_included"], reference["actual_prompt_tokens"])
                self.assertAlmostEqual(actual["modeled_bounded_tokens_included"], reference["modeled_bounded_prompt_tokens"])
                self.assertAlmostEqual(actual["modeled_difference_tokens"], reference["modeled_difference_tokens"])
                for direction in ("positive", "negative", "zero"):
                    key = f"sessions_{direction}_modeled_difference"
                    self.assertEqual(actual[key], reference[key])


if __name__ == "__main__":
    unittest.main()
