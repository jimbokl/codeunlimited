# -*- coding: utf-8 -*-
"""codeunlimited CLI: audit local agent logs, init projects for token efficiency."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from . import detectors, parsers, report
from .templates import AGENTS_BLOCK, CLAUDE_BLOCK, MARKER


def cmd_audit(args: argparse.Namespace) -> int:
    reqs = []
    if args.source in ("all", "claude"):
        reqs.extend(parsers.iter_claude())
    if args.source in ("all", "codex"):
        reqs.extend(parsers.iter_codex())
    findings = detectors.run_all(reqs)
    print(report.render(reqs, findings))
    return 0


def _append_block(path: Path, block: str) -> str:
    if path.exists():
        text = path.read_text(encoding="utf-8", errors="replace")
        if MARKER in text:
            return "already set up"
        path.write_text(text.rstrip() + "\n\n" + block, encoding="utf-8")
        return "updated"
    path.write_text(block, encoding="utf-8")
    return "created"


def cmd_init(args: argparse.Namespace) -> int:
    root = Path(args.path).resolve()
    if not root.is_dir():
        print(f"No such directory: {root}", file=sys.stderr)
        return 1
    print(f"codeunlimited init -> {root}")
    print(f"  CLAUDE.md: {_append_block(root / 'CLAUDE.md', CLAUDE_BLOCK)}")
    print(f"  AGENTS.md: {_append_block(root / 'AGENTS.md', AGENTS_BLOCK)}")
    print("Done. Claude Code and Codex pick the rules up automatically.")
    print("Check the effect in a week: codeunlimited audit")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="codeunlimited",
        description="More code out of the limits you already pay for: offline "
                    "audit of Claude Code & Codex token usage + project "
                    "efficiency setup.",
    )
    sub = ap.add_subparsers(dest="cmd", required=True)

    a = sub.add_parser("audit", help="find where your limit leaks (offline, local logs only)")
    a.add_argument("--source", choices=["all", "claude", "codex"], default="all")
    a.set_defaults(func=cmd_audit)

    i = sub.add_parser("init", help="set a project up: efficiency rules into CLAUDE.md/AGENTS.md")
    i.add_argument("path", nargs="?", default=".")
    i.set_defaults(func=cmd_init)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
