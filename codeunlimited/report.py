# -*- coding: utf-8 -*-
"""Console report: findings converted into limit currency (reclaimed work)."""
from __future__ import annotations

from collections import defaultdict

from .detectors import Finding
from .parsers import Request

BAR = "=" * 64


def render(reqs: list[Request], findings: list[Finding]) -> str:
    lines: list[str] = []
    if not reqs:
        return ("No data: no Claude Code logs (~/.claude/projects) or "
                "Codex logs (~/.codex/sessions) found.")

    per_src = defaultdict(lambda: [0, 0, 0])  # reqs, prompt, out
    per_proj = defaultdict(int)
    ts = [r.ts for r in reqs if r.ts]
    for r in reqs:
        s = per_src[r.source]
        s[0] += 1
        s[1] += r.prompt_total
        s[2] += r.out
        per_proj[f"{r.source}:{r.project}"] += r.total

    total_tokens = sum(s[1] + s[2] for s in per_src.values())
    days = max((max(ts) - min(ts)).days, 1) if ts else 1
    weekly = total_tokens / days * 7          # observed weekly volume ~ limit proxy
    out_total = sum(s[2] for s in per_src.values())
    avg_out = out_total / max(len(reqs), 1)

    lines.append(BAR)
    lines.append(" CODEUNLIMITED - more code out of the limits you already pay for")
    lines.append(BAR)
    if ts:
        lines.append(f" Period: {min(ts).date()} ... {max(ts).date()}  ({days} days)")
    for src, s in sorted(per_src.items()):
        lines.append(
            f" {src:6s}: {s[0]:>6d} requests | context {s[1]/1e6:>8.0f}M tok. | "
            f"code/answers {s[2]/1e6:>6.1f}M tok."
        )
    lines.append(f" Weekly volume (limit proxy): ~{weekly/1e6:.0f}M tokens")
    lines.append("")
    lines.append(" FINDINGS - where your limit leaks (by impact):")
    lines.append("")

    reclaimed = 0
    for i, f in enumerate(f for f in findings if f.impact_tokens > 0):
        pct = 100 * (f.impact_tokens / days * 7) / max(weekly, 1)
        answers = f.impact_tokens * (out_total / max(total_tokens - out_total, 1)) / max(avg_out, 1)
        reclaimed += f.impact_tokens
        lines.append(f" {i+1}. {f.title}")
        lines.append(f"    {f.detail}")
        lines.append(
            f"    Reclaim: ~{f.impact_tokens/1e6:.0f}M tok. "
            f"(~{pct:.0f}% of weekly volume, ~{answers:.0f} extra agent replies)"
        )
        lines.append(f"    Fix: {f.fix}")
        lines.append("")

    pct_all = 100 * (reclaimed / days * 7) / max(weekly, 1)
    lines.append(BAR)
    lines.append(
        f" TOTAL reclaimable: ~{reclaimed/1e6:.0f}M tokens "
        f"~ {pct_all:.0f}% of weekly volume - that much more work fits into the same limit."
    )
    lines.append(BAR)
    lines.append(" Top projects by volume:")
    for p, t in sorted(per_proj.items(), key=lambda x: -x[1])[:8]:
        lines.append(f"   {p:44s} {t/1e6:>8.0f}M tok.")
    lines.append("")
    lines.append(" Next: codeunlimited init <project> - efficiency rules into CLAUDE.md/AGENTS.md")
    return "\n".join(lines)
