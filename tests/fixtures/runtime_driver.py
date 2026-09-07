#!/usr/bin/env python3
"""Deterministic process fixture for the stateful runtime tests."""

from __future__ import annotations

import argparse
import json
import os
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


def argument(name: str, default: str | None = None) -> str | None:
    try:
        return sys.argv[sys.argv.index(name) + 1]
    except (ValueError, IndexError):
        return default


def codex_main() -> int:
    """Speak the real Codex CLI file-output protocol for integration tests."""
    loader_prompt = sys.stdin.buffer.read()
    capture = argument("--fixture-capture")
    state_match = re.search(rb'`([^`]*state\.json)`', loader_prompt)
    if state_match is not None:
        project = pathlib.Path(argument("--cd") or ".")
        state_path = project / state_match.group(1).decode("utf-8")
        state = json.loads(state_path.read_text(encoding="utf-8"))
        revision = int(state["revision"])
        instructions_path = state_path.with_name("provider-instructions.md")
        prompt = instructions_path.read_bytes() + b"\n" + loader_prompt
    else:
        match = re.search(rb'"revision"\s*:\s*([0-9]+)', loader_prompt)
        revision = int(match.group(1)) if match is not None else -1
        prompt = loader_prompt
    if capture:
        with pathlib.Path(capture).open("a", encoding="utf-8") as stream:
            stream.write(
                json.dumps(
                    {
                        "worker": os.environ.get("CODEUNLIMITED_RUNTIME_WORKER"),
                        "prompt": prompt.decode("utf-8"),
                        "argv": sys.argv[1:],
                        "revision_found": revision >= 0,
                    }
                )
                + "\n"
            )
    if revision < 0:
        return 65
    mode = argument("--fixture-mode", "complete")
    change = argument("--fixture-change")
    if change:
        pathlib.Path(change).write_text("provider changed workspace\n", encoding="utf-8")
    if mode == "failure":
        return 7
    output = pathlib.Path(argument("--output-last-message") or "")
    if mode == "malformed":
        output.write_text("not-json", encoding="utf-8")
        return 0
    outcome = "continue" if mode in {"continue", "unknown-usage"} else str(mode)
    result = envelope(revision, outcome)
    if mode == "blocked":
        result["delta"] = {
            "blockers_replace": [
                {"id": "fixture-blocker", "blocker": "fixture requires user action"}
            ]
        }
    output.write_text(json.dumps(result, separators=(",", ":")), encoding="utf-8")
    usage: dict[str, int] = {}
    if mode != "unknown-usage":
        usage = {
            "input_tokens": 120,
            "cache_read_input_tokens": 0,
            "output_tokens": 7,
        }
    print(json.dumps({"type": "turn.completed", "usage": usage}))
    return 0


def main() -> int:
    if "--output-last-message" in sys.argv:
        return codex_main()
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", required=True)
    parser.add_argument("--revision", type=int, default=0)
    parser.add_argument("--revision-from-prompt", action="store_true")
    parser.add_argument("--outcome", default="continue")
    parser.add_argument("--capture")
    parser.add_argument("--change")
    parser.add_argument("--mutate-intent-attempt")
    parser.add_argument("--ready")
    parser.add_argument("--release")
    parser.add_argument("--handshake-timeout", type=float, default=5.0)
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
        # Binary write: text mode would translate \n to \r\n on Windows and
        # break byte-exact assertions in the tamper tests.
        pathlib.Path(args.change).write_bytes(b"changed\n")
    if args.mutate_intent_attempt:
        intent_path = pathlib.Path(args.mutate_intent_attempt)
        intent = json.loads(intent_path.read_text(encoding="utf-8"))
        intent["attempt"] += 1
        intent_path.write_text(json.dumps(intent), encoding="utf-8")
    if args.ready or args.release:
        if not args.ready or not args.release:
            return 64
        pathlib.Path(args.ready).write_text("ready\n", encoding="utf-8")
        deadline = time.monotonic() + args.handshake_timeout
        release = pathlib.Path(args.release)
        while not release.exists():
            if time.monotonic() >= deadline:
                return 75
            time.sleep(0.01)

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
