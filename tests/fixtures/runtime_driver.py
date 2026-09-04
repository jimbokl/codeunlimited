#!/usr/bin/env python3
"""Deterministic process fixture for the stateful runtime tests."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
import time


def envelope(revision: int, outcome: str = "continue") -> dict[str, object]:
    return {
        "schema_version": 1,
        "base_revision": revision,
        "outcome": outcome,
        "summary": "fixture step complete",
        "delta": {},
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", required=True)
    parser.add_argument("--revision", type=int, default=0)
    parser.add_argument("--revision-from-prompt", action="store_true")
    parser.add_argument("--outcome", default="continue")
    parser.add_argument("--capture")
    parser.add_argument("--change")
    parser.add_argument("--mutate-intent-attempt")
    parser.add_argument("--sleep", type=float, default=2.0)
    args = parser.parse_args()

    prompt = sys.stdin.buffer.read()
    revision = args.revision
    if args.revision_from_prompt:
        match = re.search(rb'"revision":([0-9]+)', prompt)
        if match is None:
            return 65
        revision = int(match.group(1))
    if args.capture:
        pathlib.Path(args.capture).write_bytes(prompt)
    if args.change:
        pathlib.Path(args.change).write_text("changed\n", encoding="utf-8")
    if args.mutate_intent_attempt:
        intent_path = pathlib.Path(args.mutate_intent_attempt)
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        intent["attempt"] += 1
        intent_path.write_text(json.dumps(intent), encoding="utf-8")

    if args.mode == "success":
        json.dump(envelope(revision, args.outcome), sys.stdout, separators=(",", ":"))
        return 0
    if args.mode == "epistemic":
        result = envelope(revision)
        if revision == 0:
            result["summary"] = "root cause remains a bounded hypothesis"
            result["delta"] = {
                "epistemic_upsert": [
                    {
                        "id": "root-cause",
                        "claim": "The fixture behavior is caused by the bounded driver",
                        "status": "hypothesis",
                        "evidence": [],
                    }
                ]
            }
        elif revision == 1:
            if b'"id":"root-cause"' not in prompt or b'"status":"hypothesis"' not in prompt:
                return 65
            result["outcome"] = "complete"
            result["summary"] = "root cause verified by the configured check"
            result["delta"] = {
                "epistemic_upsert": [
                    {
                        "id": "root-cause",
                        "claim": "The bounded driver completes the verified fixture path",
                        "status": "verified",
                        "evidence": [{"kind": "check", "revision": 2}],
                    }
                ]
            }
        else:
            return 65
        json.dump(result, sys.stdout, separators=(",", ":"))
        return 0
    if args.mode == "claude":
        json.dump(
            {
                "type": "result",
                "result": "unused",
                "structured_output": envelope(revision, args.outcome),
                "usage": {
                    "input_tokens": 101,
                    "cache_read_input_tokens": 70,
                    "output_tokens": 9,
                },
            },
            sys.stdout,
            separators=(",", ":"),
        )
        return 0
    if args.mode == "invalid":
        sys.stdout.write("not-json")
        return 0
    if args.mode == "oversized":
        sys.stdout.write("x" * (1024 * 1024 + 1))
        return 0
    if args.mode == "sleep":
        time.sleep(args.sleep)
        json.dump(envelope(revision, args.outcome), sys.stdout)
        return 0
    if args.mode == "failure":
        sys.stderr.write("PRIVATE PROVIDER ERROR BODY")
        return 7
    return 64


if __name__ == "__main__":
    raise SystemExit(main())
