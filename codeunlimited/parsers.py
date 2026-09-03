# -*- coding: utf-8 -*-
"""Parsers for local coding-agent logs -> unified Request stream.

Sources:
  - Claude Code: ~/.claude/projects/**/*.jsonl  (assistant messages with usage)
  - Codex CLI:   ~/.codex/sessions/**/*.jsonl   (event_msg/token_count payloads)

Nothing leaves the machine. Only token counts, models, timestamps and
project names are extracted - prompts and responses are never read.
"""
from __future__ import annotations

import json
import os
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Iterator, Optional


@dataclass
class Request:
    source: str          # "claude" | "codex"
    project: str
    session: str
    ts: Optional[datetime]
    model: str
    unc_in: int          # uncached input tokens (full price / full limit weight)
    cached_in: int       # tokens served from cache
    w5: int              # cache writes, 5m TTL (claude only)
    w1h: int             # cache writes, 1h TTL (claude only)
    out: int             # output tokens

    @property
    def prompt_total(self) -> int:
        return self.unc_in + self.cached_in + self.w5 + self.w1h

    @property
    def total(self) -> int:
        return self.prompt_total + self.out


def _ts(s: str) -> Optional[datetime]:
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))
    except (ValueError, AttributeError):
        return None


def claude_root() -> Path:
    return Path(os.environ.get("CLAUDE_HOME", Path.home() / ".claude")) / "projects"


def codex_root() -> Path:
    return Path(os.environ.get("CODEX_HOME", Path.home() / ".codex")) / "sessions"


def iter_claude(root: Optional[Path] = None) -> Iterator[Request]:
    root = root or claude_root()
    if not root.is_dir():
        return
    seen: set[str] = set()
    for f in root.rglob("*.jsonl"):
        project = f.parent.name
        try:
            fh = open(f, encoding="utf-8", errors="replace")
        except OSError:
            continue
        with fh:
            for line in fh:
                if '"usage"' not in line or '"assistant"' not in line:
                    continue
                try:
                    d = json.loads(line)
                except (json.JSONDecodeError, UnicodeDecodeError):
                    continue
                if d.get("type") != "assistant":
                    continue
                msg = d.get("message") or {}
                u = msg.get("usage") or {}
                mid = msg.get("id")
                if not u or (mid and mid in seen):
                    continue
                if mid:
                    seen.add(mid)
                model = msg.get("model") or "?"
                if "<synthetic>" in model:
                    continue
                cw = u.get("cache_creation_input_tokens") or 0
                cc = u.get("cache_creation") or {}
                w1h = cc.get("ephemeral_1h_input_tokens") or 0
                yield Request(
                    source="claude",
                    project=project,
                    session=d.get("sessionId") or f.stem,
                    ts=_ts(d.get("timestamp", "")),
                    model=model,
                    unc_in=u.get("input_tokens") or 0,
                    cached_in=u.get("cache_read_input_tokens") or 0,
                    w5=cc.get("ephemeral_5m_input_tokens", cw - w1h) or 0,
                    w1h=w1h,
                    out=u.get("output_tokens") or 0,
                )


def iter_codex(root: Optional[Path] = None) -> Iterator[Request]:
    root = root or codex_root()
    if not root.is_dir():
        return
    for f in root.rglob("*.jsonl"):
        model = "?"
        project = "?"
        try:
            fh = open(f, encoding="utf-8", errors="replace")
        except OSError:
            continue
        with fh:
            for line in fh:
                if '"model":"' in line and model == "?":
                    i = line.find('"model":"') + 9
                    model = line[i:line.find('"', i)]
                if '"cwd":"' in line and project == "?":
                    i = line.find('"cwd":"') + 7
                    cwd = line[i:line.find('"', i)].replace("\\\\", "\\")
                    parts = [p for p in cwd.split("\\") if p]
                    project = parts[-1] if parts else "?"
                if '"token_count"' not in line:
                    continue
                try:
                    d = json.loads(line)
                except (json.JSONDecodeError, UnicodeDecodeError):
                    continue
                p = d.get("payload") or {}
                if p.get("type") != "token_count":
                    continue
                u = (p.get("info") or {}).get("last_token_usage") or {}
                if not u:
                    continue
                inp = u.get("input_tokens") or 0
                cch = u.get("cached_input_tokens") or 0
                yield Request(
                    source="codex",
                    project=project,
                    session=f.stem,
                    ts=_ts(d.get("timestamp", "")),
                    model=model,
                    unc_in=max(inp - cch, 0),
                    cached_in=cch,
                    w5=u.get("cache_write_input_tokens") or 0,
                    w1h=0,
                    out=u.get("output_tokens") or 0,
                )
