# -*- coding: utf-8 -*-
"""Detectors: find where subscription-limit tokens leak, in limit currency.

Every finding carries `impact_tokens` - prompt-side tokens that could have
been avoided - plus a concrete fix. The report converts impact into
"% of your weekly volume" so subscribers see reclaimed work, not dollars.
"""
from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
from typing import Iterable

from .parsers import Request

HEAVY = ("fable", "mythos", "opus")          # top-tier claude models
TRIVIAL_OUT = 300                            # output tokens: "mechanical" reply
LONG_SESSION = 30                            # requests
EARLY_N = 5


@dataclass
class Finding:
    key: str
    title: str
    impact_tokens: int
    detail: str
    fix: str
    extras: dict = field(default_factory=dict)


def _sessions(reqs: list[Request]) -> dict[tuple, list[Request]]:
    by = defaultdict(list)
    for r in reqs:
        by[(r.source, r.project, r.session)].append(r)
    for rows in by.values():
        rows.sort(key=lambda r: (r.ts is None, r.ts))
    return by


def heavy_model_on_trivial(reqs: list[Request]) -> Finding:
    """Top-tier model spent on short mechanical replies."""
    n = toks = 0
    for r in reqs:
        if r.out and r.out < TRIVIAL_OUT and any(h in r.model for h in HEAVY):
            n += 1
            toks += r.prompt_total
    return Finding(
        key="heavy_trivial",
        title="Top-tier model burned on mechanical replies",
        impact_tokens=int(toks * 0.5),  # conservative: half realistically delegable
        detail=(
            f"{n} requests to top-tier models ended in a reply shorter than "
            f"{TRIVIAL_OUT} tokens while dragging {toks/1e6:.0f}M tokens of context."
        ),
        fix=(
            "Delegate mechanical work (renames, repetitive edits, status checks) "
            "to subagents on a light model / low effort: add a delegation rule to "
            "CLAUDE.md; in Claude Code use Task with model: haiku."
        ),
    )


def context_tax(reqs: list[Request]) -> Finding:
    """Long append-only sessions drag ever-growing context through every turn."""
    excess = 0
    sessions_hit = 0
    growth_samples = []
    for rows in _sessions(reqs).values():
        if len(rows) < LONG_SESSION:
            continue
        early = rows[:EARLY_N]
        early_avg = sum(r.prompt_total for r in early) / max(len(early), 1)
        late = rows[LONG_SESSION:]
        e = sum(max(r.prompt_total - early_avg, 0) for r in late)
        if e > 0:
            sessions_hit += 1
            excess += int(e)
            late_avg = sum(r.prompt_total for r in late) / max(len(late), 1)
            if early_avg > 0:
                growth_samples.append(late_avg / early_avg)
    growth = sum(growth_samples) / len(growth_samples) if growth_samples else 0
    return Finding(
        key="context_tax",
        title="Context tax of long sessions",
        impact_tokens=int(excess * 0.6),  # conservative: part of context is genuinely needed
        detail=(
            f"{sessions_hit} sessions ran past {LONG_SESSION} turns; by the tail of "
            f"a session each turn costs on average x{growth:.1f} of an early turn."
        ),
        fix=(
            "New task = new session (/clear). For long repetitive loops keep a "
            "compact state file instead of conversation history (SKILL.state "
            "pattern, arXiv 2608.26263) - `codeunlimited init` adds the rule to "
            "CLAUDE.md."
        ),
        extras={"sessions": sessions_hit, "growth": growth},
    )


def cache_rewrites(reqs: list[Request]) -> Finding:
    """Mid-session full cache re-writes: broken prefix or expired TTL."""
    brk = ttl = 0
    brk_ev = ttl_ev = 0
    for rows in _sessions(reqs).values():
        for i in range(1, len(rows)):
            r = rows[i]
            if r.source != "claude" or r.cached_in > 0:
                continue
            w = r.w5 + r.w1h
            if w < 2000:
                continue
            prev = rows[i - 1]
            if r.ts and prev.ts:
                gap = (r.ts - prev.ts).total_seconds()
                limit = 3600 if (r.w1h or prev.w1h) else 300
                if gap > limit:
                    ttl += w
                    ttl_ev += 1
                    continue
            brk += w
            brk_ev += 1
    return Finding(
        key="cache_rewrites",
        title="Mid-session cache re-writes",
        impact_tokens=brk + ttl,
        detail=(
            f"{brk_ev} prefix breaks ({brk/1e6:.1f}M tok.) and {ttl_ev} TTL "
            f"expirations ({ttl/1e6:.1f}M tok.) re-paid for context instead of "
            f"reading it back from cache."
        ),
        fix=(
            "Breaks: move mutating blocks (timestamps, dynamic state) out of the "
            "prompt prefix. Expirations: avoid 5+ minute pauses mid-task."
        ),
    )


def heavy_session_start(reqs: list[Request]) -> Finding:
    """Big first-request cache write = harness overhead (tools/MCP schemas)."""
    firsts = []
    for rows in _sessions(reqs).values():
        if rows and rows[0].source == "claude":
            w = rows[0].w5 + rows[0].w1h + rows[0].unc_in
            if w:
                firsts.append(w)
    firsts.sort()
    med = firsts[len(firsts) // 2] if firsts else 0
    over = sum(max(w - 25_000, 0) for w in firsts)
    return Finding(
        key="fat_start",
        title="Fat session starts (tool/MCP schemas in the system prompt)",
        impact_tokens=int(over * 0.5),
        detail=(
            f"The median first request of a session writes {med/1000:.0f}k tokens "
            f"of context; anything above ~25k is usually schemas of unused MCP "
            f"servers and tools."
        ),
        fix=(
            "Disable unused MCP servers per project (.mcp.json / `claude mcp "
            "remove`) - their schemas are paid out of your limit on every new "
            "session."
        ),
        extras={"median_first": med},
    )


def run_all(reqs: Iterable[Request]) -> list[Finding]:
    reqs = list(reqs)
    findings = [
        heavy_model_on_trivial(reqs),
        context_tax(reqs),
        cache_rewrites(reqs),
        heavy_session_start(reqs),
    ]
    return sorted(findings, key=lambda f: -f.impact_tokens)
