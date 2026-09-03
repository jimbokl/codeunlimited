# -*- coding: utf-8 -*-
"""codeunlimited CLI: audit local agent logs, init projects for token efficiency."""
from __future__ import annotations

import argparse
import sys
from pathlib import Path

from . import detectors, parsers, report
from .templates import AGENTS_BLOCK, CLAUDE_BLOCK, MARKER


def cmd_audit(args: argparse.Namespace) -> int:
    project = Path(args.project).resolve() if args.project else None
    reqs = []
    if args.source in ("all", "claude"):
        reqs.extend(parsers.iter_claude(project=project))
    if args.source in ("all", "codex"):
        reqs.extend(parsers.iter_codex(project=project))
    findings = detectors.run_all(reqs)
    if project:
        print(f"[scope: {project}]")
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


def _baseline(root: Path) -> None:
    """Instant per-project baseline for the attach-to-existing-project case.

    Scans only this project's Claude Code log directory (fast); the full
    cross-source scoped report is `codeunlimited audit --project <path>`.
    """
    reqs = list(parsers.iter_claude(project=root))
    if not reqs:
        print("  history: none yet - new project, baseline starts now")
        return
    sessions = len({r.session for r in reqs})
    total = sum(r.total for r in reqs)
    print(f"  history: {len(reqs)} requests in {sessions} sessions "
          f"({total/1e6:.0f}M tokens) - existing project, baseline captured")
    top = [f for f in detectors.run_all(reqs) if f.impact_tokens > 0]
    if top:
        print(f"  top leak here: {top[0].title} (~{top[0].impact_tokens/1e6:.0f}M tok. reclaimable)")
    print(f"  full scoped report: codeunlimited audit --project \"{root}\"")


def cmd_init(args: argparse.Namespace) -> int:
    root = Path(args.path).resolve()
    if not root.is_dir():
        print(f"No such directory: {root}", file=sys.stderr)
        return 1
    print(f"codeunlimited init -> {root}")
    print(f"  CLAUDE.md: {_append_block(root / 'CLAUDE.md', CLAUDE_BLOCK)}")
    print(f"  AGENTS.md: {_append_block(root / 'AGENTS.md', AGENTS_BLOCK)}")
    _baseline(root)
    print("Done. Claude Code and Codex pick the rules up automatically.")
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
    a.add_argument("--project", metavar="PATH",
                   help="scope the report to one project directory")
    a.set_defaults(func=cmd_audit)

    i = sub.add_parser(
        "init",
        help="works both for a brand-new project and for attaching to an "
             "existing one: writes efficiency rules into CLAUDE.md/AGENTS.md "
             "and, if the project already has history, prints its baseline",
    )
    i.add_argument("path", nargs="?", default=".")
    i.set_defaults(func=cmd_init)

    args = ap.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
