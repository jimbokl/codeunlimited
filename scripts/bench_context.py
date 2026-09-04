"""Deterministic context-tax benchmark on real Claude Code logs.

For every session > MIN_TURNS turns: actual prompt tokens processed vs the
bounded-context cost (same number of turns, each at the session's own
early-turn average context = what a fresh session / state-file loop pays).
No estimates: both numbers are exact sums over the user's own log records.
"""
import json, os, glob, sys
from collections import defaultdict

ROOT = os.path.expanduser(r"~\.claude\projects")
MIN_TURNS = 30
EARLY = 5

sessions = defaultdict(list)  # (project, sessionId) -> [(ts, prompt_total)]
seen = set()
for path in glob.glob(os.path.join(ROOT, "*", "*.jsonl")):
    proj = os.path.basename(os.path.dirname(path))
    try:
        with open(path, encoding="utf-8", errors="ignore") as f:
            for line in f:
                if '"usage"' not in line or '"assistant"' not in line:
                    continue
                try:
                    d = json.loads(line)
                except Exception:
                    continue
                if d.get("type") != "assistant":
                    continue
                m = d.get("message") or {}
                u = m.get("usage") or {}
                if not u or "<synthetic>" in str(m.get("model", "")):
                    continue
                mid = m.get("id")
                if mid:
                    if mid in seen:
                        continue
                    seen.add(mid)
                prompt = (u.get("input_tokens", 0) + u.get("cache_read_input_tokens", 0)
                          + u.get("cache_creation_input_tokens", 0))
                sessions[(proj, d.get("sessionId", "?"))].append(
                    (d.get("timestamp", ""), prompt))
    except OSError:
        continue

rows = []
for (proj, sid), turns in sessions.items():
    if len(turns) < MIN_TURNS:
        continue
    turns.sort()
    vals = [p for _, p in turns]
    early_avg = sum(vals[:EARLY]) / EARLY
    actual = sum(vals)
    bounded = early_avg * len(vals)
    if bounded > 0 and actual > bounded:
        rows.append((actual / bounded, actual, bounded, len(vals), proj))

rows.sort(reverse=True)
ta = sum(r[1] for r in rows)
tb = sum(r[2] for r in rows)
print(f"long sessions (> {MIN_TURNS} turns): {len(rows)}")
print(f"actual prompt tokens processed : {ta/1e6:10.0f}M")
print(f"bounded-context cost (exact)   : {tb/1e6:10.0f}M")
print(f"overall multiplier             : x{ta/max(tb,1):.1f}")
print(f"tokens burned by context growth: {(ta-tb)/1e6:10.0f}M")
print("\ntop sessions (multiplier / actual / bounded / turns / project):")
for m, a, b, n, proj in rows[:8]:
    print(f"  x{m:5.1f}  {a/1e6:8.0f}M -> {b/1e6:6.0f}M  {n:5} turns  {proj}")
