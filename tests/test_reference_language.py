import datetime as dt
import pathlib
import unittest

from codeunlimited.detectors import context_tax
from codeunlimited.parsers import Request
from codeunlimited.templates import AGENTS_BLOCK, CLAUDE_BLOCK


class ReferenceLanguageTests(unittest.TestCase):
    def test_session_guidance_is_conditional_on_context_reuse(self) -> None:
        rows = [
            Request(
                source="claude",
                project="fixture",
                session="session",
                ts=dt.datetime(2026, 1, 1, tzinfo=dt.timezone.utc)
                + dt.timedelta(minutes=index),
                model="claude-opus",
                unc_in=10_000 + index * 1_000,
                cached_in=0,
                w5=0,
                w1h=0,
                out=100,
            )
            for index in range(31)
        ]

        finding = context_tax(rows)
        for text in (finding.fix, CLAUDE_BLOCK, AGENTS_BLOCK):
            self.assertIn("Batch small related tasks", text)
            self.assertIn("prior context", text)
            self.assertNotIn("New task = new session", text)

    def test_handoff_labels_the_context_counterfactual_as_modeled(self) -> None:
        root = pathlib.Path(__file__).resolve().parents[1]
        handoff = (root / "docs" / "HANDOFF-CODEX.md").read_text(encoding="utf-8")

        self.assertRegex(handoff, r"modeled\s+bounded-context counterfactual")
        self.assertNotIn("bounded-context vs actual", handoff)


if __name__ == "__main__":
    unittest.main()
