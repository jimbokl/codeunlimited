#!/usr/bin/env python3
"""Audit observed context growth against an explicit counterfactual model.

Observed prompt totals are exact sums of recognized local log counters. The
bounded-context total is modeled from each session's early-request mean; it is
not a measured or causal savings figure.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from collections import defaultdict
from dataclasses import dataclass
from typing import Iterable


SessionKey = tuple[str, str]
Turn = tuple[str, int]


@dataclass(frozen=True)
class LoadResult:
    sessions: dict[SessionKey, list[Turn]]
    files_read: int
    recognized_records: int
    malformed_candidates: int


def default_root() -> pathlib.Path:
    return pathlib.Path.home() / ".claude" / "projects"


def _token(value: object) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError("token counter must be a non-negative integer")
    return value


def load_sessions(root: pathlib.Path) -> LoadResult:
    root = root.expanduser()
    if not root.is_dir():
        raise FileNotFoundError("Claude projects root does not exist")

    sessions: dict[SessionKey, list[Turn]] = defaultdict(list)
    seen: set[tuple[SessionKey, str]] = set()
    files_read = 0
    recognized = 0
    malformed = 0

    for path in sorted(root.glob("*/*.jsonl")):
        files_read += 1
        project = path.parent.name
        with path.open(encoding="utf-8") as stream:
            for line in stream:
                if '"usage"' not in line or '"assistant"' not in line:
                    continue
                try:
                    record = json.loads(line)
                except (json.JSONDecodeError, TypeError):
                    malformed += 1
                    continue
                if not isinstance(record, dict):
                    malformed += 1
                    continue
                if record.get("type") != "assistant":
                    continue
                message = record.get("message")
                usage = message.get("usage") if isinstance(message, dict) else None
                if not isinstance(usage, dict):
                    malformed += 1
                    continue
                if "<synthetic>" in str(message.get("model", "")):
                    continue
                try:
                    prompt = sum(
                        _token(usage.get(field, 0))
                        for field in (
                            "input_tokens",
                            "cache_read_input_tokens",
                            "cache_creation_input_tokens",
                        )
                    )
                except ValueError:
                    malformed += 1
                    continue

                session = str(record.get("sessionId") or path.stem)
                key = (project, session)
                message_id = message.get("id")
                if message_id:
                    identity = (key, str(message_id))
                    if identity in seen:
                        continue
                    seen.add(identity)
                sessions[key].append((str(record.get("timestamp") or ""), prompt))
                recognized += 1

    return LoadResult(dict(sessions), files_read, recognized, malformed)


def analyze(
    sessions: dict[SessionKey, list[Turn]],
    *,
    min_turns: int,
    early_turns: int,
    malformed_candidates: int = 0,
) -> dict[str, object]:
    if min_turns < 0:
        raise ValueError("min_turns must be non-negative")
    if early_turns < 1:
        raise ValueError("early_turns must be positive")

    actual_total = 0
    modeled_total = 0.0
    included = 0
    positive = 0
    negative = 0
    zero = 0

    for turns in sessions.values():
        if len(turns) <= min_turns:
            continue
        values = [prompt for _, prompt in sorted(turns)]
        sample_count = min(early_turns, len(values))
        early_mean = sum(values[:sample_count]) / sample_count
        actual = sum(values)
        modeled = early_mean * len(values)
        difference = actual - modeled
        included += 1
        actual_total += actual
        modeled_total += modeled
        if difference > 0:
            positive += 1
        elif difference < 0:
            negative += 1
        else:
            zero += 1

    return {
        "schema_version": 1,
        "method": "observed prompt sum versus modeled early-context counterfactual",
        "min_turns_exclusive": min_turns,
        "early_turns": early_turns,
        "sessions_discovered": len(sessions),
        "sessions_included": included,
        "sessions_positive_modeled_difference": positive,
        "sessions_negative_modeled_difference": negative,
        "sessions_zero_modeled_difference": zero,
        "actual_prompt_tokens": actual_total,
        "modeled_bounded_prompt_tokens": modeled_total,
        "modeled_difference_tokens": actual_total - modeled_total,
        "aggregate_multiplier": actual_total / modeled_total if modeled_total else None,
        "malformed_candidate_records": malformed_candidates,
        "complete_accounting": malformed_candidates == 0,
    }


def render_text(result: dict[str, object]) -> str:
    multiplier = result["aggregate_multiplier"]
    multiplier_text = "n/a" if multiplier is None else f"x{float(multiplier):.2f}"
    completeness = "complete" if result["complete_accounting"] else "incomplete"
    lines = [
        f"eligible sessions (> {result['min_turns_exclusive']} turns): {result['sessions_included']}",
        f"actual prompt tokens (exact observed) : {int(result['actual_prompt_tokens']):,}",
        f"bounded prompt tokens (modeled)       : {float(result['modeled_bounded_prompt_tokens']):,.1f}",
        f"difference (modeled counterfactual)   : {float(result['modeled_difference_tokens']):,.1f}",
        f"aggregate actual/model multiplier     : {multiplier_text}",
        "session directions (positive/negative/zero): "
        f"{result['sessions_positive_modeled_difference']}/"
        f"{result['sessions_negative_modeled_difference']}/"
        f"{result['sessions_zero_modeled_difference']}",
        f"accounting: {completeness}; malformed candidates: {result['malformed_candidate_records']}",
    ]
    return "\n".join(lines)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=pathlib.Path, default=default_root())
    parser.add_argument("--min-turns", type=int, default=30)
    parser.add_argument("--early-turns", type=int, default=5)
    parser.add_argument("--json", action="store_true", dest="json_output")
    return parser


def _safe_input_error(error: OSError | UnicodeError) -> str:
    if isinstance(error, UnicodeError):
        return "a log file is not valid UTF-8"
    if isinstance(error, FileNotFoundError):
        return "Claude projects root does not exist"
    if isinstance(error, PermissionError):
        return "a log file cannot be read (permission denied)"
    return f"log input could not be read ({type(error).__name__})"


def main(argv: Iterable[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(list(argv) if argv is not None else None)
    if args.min_turns < 0:
        parser.error("--min-turns must be non-negative")
    if args.early_turns < 1:
        parser.error("--early-turns must be positive")
    try:
        loaded = load_sessions(args.root)
    except (OSError, UnicodeError) as error:
        print(f"benchmark failed: {_safe_input_error(error)}", file=sys.stderr)
        return 2
    result = analyze(
        loaded.sessions,
        min_turns=args.min_turns,
        early_turns=args.early_turns,
        malformed_candidates=loaded.malformed_candidates,
    )
    result["files_read"] = loaded.files_read
    result["recognized_records"] = loaded.recognized_records
    if args.json_output:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(render_text(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
