#!/usr/bin/env python3
"""Deterministic file-editing worker and verifier for packet runtime tests."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys


EXPECTED = {
    "unit-a": ("units/a.txt", "alpha\n"),
    "unit-b": ("units/b.txt", "bravo\n"),
    "unit-c": ("units/c.txt", "charlie\n"),
    "unit-d": ("units/d.txt", "delta\n"),
}


def selected_packet(prompt: bytes) -> list[dict[str, object]]:
    match = re.search(rb"SELECTED_PACKET\n([^\n]+)\nEND_SELECTED_PACKET", prompt)
    if match is None:
        raise ValueError("selected packet missing")
    value = json.loads(match.group(1))
    if not isinstance(value, list):
        raise ValueError("selected packet is not an array")
    return value


def revision(prompt: bytes) -> int:
    match = re.search(rb'"revision":([0-9]+)', prompt)
    if match is None:
        raise ValueError("revision missing")
    return int(match.group(1))


def write_unit(task_id: str, wrong: bool = False) -> None:
    relative, contents = EXPECTED[task_id]
    path = pathlib.Path(relative)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("wrong\n" if wrong else contents, encoding="utf-8")


def worker(mode: str, block_attempt_record: str | None = None) -> int:
    prompt = sys.stdin.buffer.read()
    if block_attempt_record is not None:
        pathlib.Path(block_attempt_record).mkdir()
    packet = selected_packet(prompt)
    ids = [str(task["id"]) for task in packet]
    accepted = ids
    outcome = "continue"
    queue_replace = None

    if mode == "full":
        for task_id in ids:
            write_unit(task_id)
        outcome = "complete" if ids and ids[-1] == "unit-d" else "continue"
    elif mode == "partial":
        accepted = ids[:2]
        for task_id in accepted:
            write_unit(task_id)
    elif mode == "wrong-verification":
        write_unit(ids[0], wrong=True)
        accepted = ids[:1]
    elif mode in {"invalid", "skip-prefix"}:
        accepted = ids[1:2]
    elif mode == "rewrite-task":
        accepted = ids[:1]
        queue_replace = [
            {"id": str(task["id"]), "task": str(task["task"])} for task in packet[1:]
        ]
        queue_replace[0]["task"] = "rewritten by worker"
    elif mode == "premature-complete":
        accepted = ids[:1]
        outcome = "complete"
    elif mode == "final-continue":
        accepted = ids
    elif mode == "final-blocked":
        accepted = ids
        outcome = "blocked"
    elif mode == "blocked-partial":
        accepted = ids[:2]
        outcome = "blocked"
    elif mode == "blocked":
        accepted = []
        outcome = "blocked"
    else:
        return 64

    delta: dict[str, object] = {
        "completed_add": [
            {"id": task_id, "result": "fixture accepted"} for task_id in accepted
        ]
    }
    if queue_replace is not None:
        delta["queue_replace"] = queue_replace
    if outcome == "blocked":
        delta["blockers_replace"] = [
            {"id": "fixture-blocker", "blocker": "fixture explicitly blocked"}
        ]
    json.dump(
        {
            "schema_version": 1,
            "base_revision": revision(prompt),
            "outcome": outcome,
            "summary": "packet fixture response",
            "delta": delta,
        },
        sys.stdout,
        separators=(",", ":"),
    )
    return 0


def verify(capture: str | None) -> int:
    if capture is not None:
        pathlib.Path(capture).write_text("verification invoked\n", encoding="utf-8")
    for task_id, (relative, contents) in EXPECTED.items():
        path = pathlib.Path(relative)
        if path.exists() and path.read_text(encoding="utf-8") != contents:
            print(f"invalid {task_id}", file=sys.stderr)
            return 1
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--capture")
    parser.add_argument("--block-attempt-record")
    args = parser.parse_args()
    if args.verify:
        return verify(args.capture)
    if args.mode is None:
        return 64
    return worker(args.mode, args.block_attempt_record)


if __name__ == "__main__":
    raise SystemExit(main())
