#!/usr/bin/env python3
"""Deterministic process fixture for the stateful runtime tests."""

from __future__ import annotations

import argparse
import json
import pathlib
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
    parser.add_argument("--capture")
    parser.add_argument("--change")
    parser.add_argument("--sleep", type=float, default=2.0)
    args = parser.parse_args()

    prompt = sys.stdin.buffer.read()
    if args.capture:
        pathlib.Path(args.capture).write_bytes(prompt)
    if args.change:
        pathlib.Path(args.change).write_text("changed\n", encoding="utf-8")

    if args.mode == "success":
        json.dump(envelope(args.revision), sys.stdout, separators=(",", ":"))
        return 0
    if args.mode == "claude":
        json.dump(
            {
                "type": "result",
                "result": "unused",
                "structured_output": envelope(args.revision),
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
        json.dump(envelope(args.revision), sys.stdout)
        return 0
    if args.mode == "failure":
        sys.stderr.write("PRIVATE PROVIDER ERROR BODY")
        return 7
    return 64


if __name__ == "__main__":
    raise SystemExit(main())
