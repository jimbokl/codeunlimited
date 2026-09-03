#!/usr/bin/env python3
"""Run redacted, reproducible local codeunlimited performance scenarios."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import os
import pathlib
import platform
import statistics
import subprocess
import sys
import tempfile
import time
from collections.abc import Mapping, Sequence
from typing import Any


SCAN_KEYS = (
    "files_discovered",
    "files_opened",
    "files_skipped_by_date",
    "files_skipped_by_index",
    "usage_records",
)
SOURCE_KEYS = ("claude", "codex")


def nearest_rank(values: Sequence[float], quantile: float) -> float:
    """Return an observed quantile using the nearest-rank definition."""
    if not values:
        raise ValueError("nearest_rank requires at least one value")
    if not 0 < quantile <= 1:
        raise ValueError("quantile must be in (0, 1]")
    ordered = sorted(values)
    rank = max(1, math.ceil(quantile * len(ordered)))
    return ordered[rank - 1]


def summarize(samples: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    walls = [float(sample["wall_seconds"]) for sample in samples]
    rss_values = [
        int(sample["max_rss_bytes"])
        for sample in samples
        if sample.get("max_rss_bytes") is not None
    ]
    return {
        "wall_seconds": {
            "median": round(statistics.median(walls), 6),
            "p95": round(nearest_rank(walls, 0.95), 6),
        },
        "max_rss_bytes": max(rss_values) if rss_values else None,
    }


def _rss_from_time(stderr: str, system: str) -> int | None:
    for line in stderr.splitlines():
        if system == "Darwin" and "maximum resident set size" in line:
            value = line.strip().split(maxsplit=1)[0]
            if value.isdigit():
                return int(value)
        if system == "Linux" and line.strip().startswith(
            "Maximum resident set size (kbytes):"
        ):
            value = line.rsplit(":", 1)[-1].strip()
            if value.isdigit():
                return int(value) * 1024
    return None


def _safe_audit_fields(stdout: str) -> tuple[dict[str, int], dict[str, int], bool]:
    try:
        payload = json.loads(stdout)
    except (json.JSONDecodeError, TypeError):
        return {}, {}, False
    if not isinstance(payload, dict):
        return {}, {}, False

    source_requests: dict[str, int] = {}
    sources = payload.get("sources")
    if isinstance(sources, dict):
        for source in SOURCE_KEYS:
            details = sources.get(source)
            if isinstance(details, dict) and isinstance(details.get("requests"), int):
                source_requests[source] = details["requests"]

    scan_counters: dict[str, int] = {}
    scan = payload.get("scan")
    if isinstance(scan, dict):
        for key in SCAN_KEYS:
            if isinstance(scan.get(key), int):
                scan_counters[key] = scan[key]
    return source_requests, scan_counters, True


def _measure(command: Sequence[str], env: Mapping[str, str]) -> dict[str, Any]:
    system = platform.system()
    timer = pathlib.Path("/usr/bin/time")
    if timer.is_file() and system == "Darwin":
        measured_command = [str(timer), "-lp", *command]
    elif timer.is_file() and system == "Linux":
        measured_command = [str(timer), "-v", *command]
    else:
        measured_command = list(command)

    child_env = os.environ.copy()
    child_env.update(env)
    started = time.perf_counter()
    try:
        completed = subprocess.run(
            measured_command,
            check=False,
            capture_output=True,
            text=True,
            env=child_env,
        )
    except OSError:
        return {
            "exit_code": None,
            "wall_seconds": round(time.perf_counter() - started, 6),
            "max_rss_bytes": None,
            "json_valid": False,
            "source_requests": {},
            "scan": {},
        }
    wall_seconds = time.perf_counter() - started
    source_requests, scan, json_valid = _safe_audit_fields(completed.stdout)
    return {
        "exit_code": completed.returncode,
        "wall_seconds": round(wall_seconds, 6),
        "max_rss_bytes": _rss_from_time(completed.stderr, system),
        "json_valid": json_valid,
        "source_requests": source_requests,
        "scan": scan,
    }


def run_scenario(
    name: str,
    command: Sequence[str],
    env: Mapping[str, str],
    runs: int,
    *,
    warmup: bool = False,
) -> dict[str, Any]:
    if runs < 1:
        raise ValueError("runs must be at least 1")
    warmup_sample = _measure(command, env) if warmup else None
    samples = [_measure(command, env) for _ in range(runs)]
    warmup_ok = warmup_sample is None or (
        warmup_sample["exit_code"] == 0 and warmup_sample["json_valid"]
    )
    successful = warmup_ok and all(
        sample["exit_code"] == 0 and sample["json_valid"] for sample in samples
    )
    result: dict[str, Any] = {
        "name": name,
        "status": "ok" if successful else "failed",
        "samples": samples,
        "summary": summarize(samples),
    }
    if warmup_sample is not None:
        result["warmup_exit_code"] = warmup_sample["exit_code"]
    return result


def write_output(path: pathlib.Path, payload: Mapping[str, Any], force: bool) -> None:
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    encoded = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    if not force:
        with path.open("x", encoding="utf-8") as stream:
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        return

    temporary: pathlib.Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as stream:
            temporary = pathlib.Path(stream.name)
            stream.write(encoded)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def _audit_command(binary: pathlib.Path, *args: str) -> list[str]:
    return [str(binary), "audit", "--json", "--scan-stats", *args]


def _codeunlimited_version(binary: pathlib.Path) -> str:
    try:
        completed = subprocess.run(
            [str(binary), "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return "unknown"
    for line in (completed.stdout + "\n" + completed.stderr).splitlines():
        parts = line.strip().split()
        if len(parts) == 2 and parts[0] == "codeunlimited":
            return parts[1]
    return "unknown"


def _git_sha(root: pathlib.Path) -> str:
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "HEAD"],
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return "unknown"
    sha = completed.stdout.strip().lower()
    if completed.returncode == 0 and len(sha) == 40 and all(
        character in "0123456789abcdef" for character in sha
    ):
        return sha
    return "unknown"


def _total_memory_bytes() -> int | None:
    system = platform.system()
    if system == "Darwin":
        try:
            completed = subprocess.run(
                ["/usr/sbin/sysctl", "-n", "hw.memsize"],
                check=False,
                capture_output=True,
                text=True,
                timeout=10,
            )
            value = completed.stdout.strip()
            if completed.returncode == 0 and value.isdigit():
                return int(value)
        except (OSError, subprocess.TimeoutExpired):
            return None
    if system == "Linux":
        try:
            pages = os.sysconf("SC_PHYS_PAGES")
            page_size = os.sysconf("SC_PAGE_SIZE")
            if isinstance(pages, int) and isinstance(page_size, int):
                return pages * page_size
        except (OSError, ValueError):
            return None
    return None


def _corpus_metadata() -> dict[str, int]:
    home = pathlib.Path.home()
    roots = (
        pathlib.Path(os.environ.get("CLAUDE_HOME", home / ".claude")) / "projects",
        pathlib.Path(os.environ.get("CODEX_HOME", home / ".codex")) / "sessions",
    )
    files = 0
    total_bytes = 0
    for root in roots:
        if not root.is_dir():
            continue
        try:
            candidates = root.rglob("*.jsonl")
            for path in candidates:
                try:
                    if path.is_file():
                        files += 1
                        total_bytes += path.stat().st_size
                except OSError:
                    continue
        except OSError:
            continue
    return {"jsonl_files": files, "bytes": total_bytes}


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--days", type=int, default=30)
    parser.add_argument("--project", type=pathlib.Path)
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--force", action="store_true")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = _parser()
    args = parser.parse_args(argv)
    if args.runs < 1:
        parser.error("--runs must be at least 1")
    if args.days < 1:
        parser.error("--days must be at least 1")
    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"binary does not exist: {binary}")
    if args.output is not None and args.output.exists() and not args.force:
        print(f"benchmark output already exists: {args.output}", file=sys.stderr)
        return 2

    root = pathlib.Path(__file__).resolve().parents[1]
    project = (args.project or root).resolve()
    fixtures = root / "tests" / "fixtures"
    scenarios: list[dict[str, Any]] = []
    with tempfile.TemporaryDirectory(prefix="codeunlimited-benchmark-") as state:
        shared_env = {"CODEUNLIMITED_HOME": state}
        fixture_env = {
            **shared_env,
            "CLAUDE_HOME": str(fixtures / "claude_home"),
            "CODEX_HOME": str(fixtures / "codex_home"),
        }
        scenarios.append(
            run_scenario(
                "fixture",
                _audit_command(binary, "--no-index"),
                fixture_env,
                args.runs,
            )
        )
        scenarios.append(
            run_scenario(
                "full_no_index",
                _audit_command(binary, "--no-index"),
                shared_env,
                args.runs,
            )
        )
        scenarios.append(
            run_scenario(
                "bounded_time_warm_index",
                _audit_command(binary, "--days", str(args.days)),
                shared_env,
                args.runs,
                warmup=True,
            )
        )
        scenarios.append(
            run_scenario(
                "scoped_no_index",
                _audit_command(binary, "--project", str(project), "--no-index"),
                shared_env,
                args.runs,
            )
        )
        scenarios.append(
            run_scenario(
                "scoped_warm_index",
                _audit_command(binary, "--project", str(project)),
                shared_env,
                args.runs,
                warmup=True,
            )
        )

    payload = {
        "schema_version": 1,
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "provenance": {
            "codeunlimited_version": _codeunlimited_version(binary),
            "git_sha": _git_sha(root),
        },
        "platform": {
            "system": platform.system(),
            "machine": platform.machine(),
            "python": platform.python_version(),
            "total_memory_bytes": _total_memory_bytes(),
        },
        "corpus": _corpus_metadata(),
        "runs_per_scenario": args.runs,
        "days": args.days,
        "scenarios": scenarios,
    }
    if args.output is not None:
        try:
            write_output(args.output, payload, args.force)
        except OSError as error:
            print(f"cannot write benchmark output: {error}", file=sys.stderr)
            return 2
    else:
        print(json.dumps(payload, indent=2, sort_keys=True))
    return 0 if all(item["status"] == "ok" for item in scenarios) else 1


if __name__ == "__main__":
    raise SystemExit(main())
