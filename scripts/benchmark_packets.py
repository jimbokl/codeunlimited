#!/usr/bin/env python3
"""Run the deterministic packet fixture through the public codeunlimited CLI."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
from typing import Any, Sequence


ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "fixtures" / "packet_driver.py"
TASK_IDS = ["unit-a", "unit-b", "unit-c", "unit-d"]
EXPECTED_FILES = {
    "units/a.txt": b"alpha\n",
    "units/b.txt": b"bravo\n",
    "units/c.txt": b"charlie\n",
    "units/d.txt": b"delta\n",
}
COMMAND_TIMEOUT_SECONDS = 30


class BenchmarkError(RuntimeError):
    """A deterministic fixture command violated the benchmark contract."""


def run(
    argv: Sequence[str | pathlib.Path],
    *,
    cwd: pathlib.Path,
    text: bool = False,
) -> subprocess.CompletedProcess[Any]:
    result = subprocess.run(
        [str(value) for value in argv],
        cwd=cwd,
        check=False,
        capture_output=True,
        text=text,
        timeout=COMMAND_TIMEOUT_SECONDS,
    )
    if result.returncode != 0:
        stderr = result.stderr if text else result.stderr.decode("utf-8", "replace")
        raise BenchmarkError(f"command failed with exit {result.returncode}: {stderr.strip()}")
    return result


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(64 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def final_tree_digest(project: pathlib.Path) -> str:
    """Hash the exact expected path/content set with unambiguous lengths."""
    unit_files = sorted(
        path.relative_to(project).as_posix()
        for path in (project / "units").rglob("*")
        if path.is_file()
    )
    expected_paths = sorted(EXPECTED_FILES)
    if unit_files != expected_paths:
        raise BenchmarkError("fixture did not produce the exact expected file set")

    digest = hashlib.sha256()
    for relative in expected_paths:
        path_bytes = relative.encode("utf-8")
        contents = (project / relative).read_bytes()
        if contents != EXPECTED_FILES[relative]:
            raise BenchmarkError(f"fixture produced unexpected bytes for {relative}")
        digest.update(len(path_bytes).to_bytes(4, "big"))
        digest.update(path_bytes)
        digest.update(len(contents).to_bytes(8, "big"))
        digest.update(contents)
    return digest.hexdigest()


def initialize_git(project: pathlib.Path) -> None:
    run(["git", "init", "-q"], cwd=project)
    run(["git", "add", "."], cwd=project)
    run(
        [
            "git",
            "-c",
            "user.name=Packet Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture baseline",
        ],
        cwd=project,
    )


def plan(packet_cap: int) -> dict[str, object]:
    scope = ["units/a.txt", "units/b.txt", "units/c.txt", "units/d.txt"]
    return {
        "schema_version": 1,
        "max_packet_tasks": packet_cap,
        "tasks": [
            {
                "id": "unit-a",
                "task": "Write alpha to units/a.txt",
                "group": "units",
                "depends_on": [],
                "scope": scope,
                "risk": "low",
            },
            {
                "id": "unit-b",
                "task": "Write bravo to units/b.txt",
                "group": "units",
                "depends_on": ["unit-a"],
                "scope": scope,
                "risk": "low",
            },
            {
                "id": "unit-c",
                "task": "Write charlie to units/c.txt",
                "group": "units",
                "depends_on": ["unit-b"],
                "scope": scope,
                "risk": "low",
            },
            {
                "id": "unit-d",
                "task": "Write delta to units/d.txt",
                "group": "units",
                "depends_on": ["unit-c"],
                "scope": scope,
                "risk": "low",
            },
        ],
    }


def write_fixture_inputs(project: pathlib.Path, packet_cap: int) -> tuple[pathlib.Path, pathlib.Path]:
    workflow = project / "workflow.md"
    plan_path = project / "plan.json"
    workflow.write_text(
        "# Offline packet fixture\nComplete only the selected deterministic tasks.\n",
        encoding="utf-8",
    )
    plan_path.write_text(
        json.dumps(plan(packet_cap), sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    return workflow, plan_path


def benchmark_arm(
    binary: pathlib.Path,
    project: pathlib.Path,
    packet_cap: int,
) -> dict[str, object]:
    project.mkdir()
    workflow, plan_path = write_fixture_inputs(project, packet_cap)
    run_name = "fixture"
    run(
        [
            binary,
            "run",
            "init",
            run_name,
            "--project",
            project,
            "--skill",
            workflow,
            "--objective",
            "Produce the four literal offline fixture files",
            "--provider",
            "command",
            "--provider-executable",
            sys.executable,
            f"--provider-arg={FIXTURE}",
            "--provider-arg=--mode",
            "--provider-arg=full",
            "--work-plan",
            plan_path,
            "--verify-program",
            sys.executable,
            f"--verify-arg={FIXTURE}",
            "--verify-arg=--verify",
        ],
        cwd=project,
    )
    initialize_git(project)

    prompt_bytes_total = 0
    worker_process_count = 0
    terminal_status = ""
    for _ in range(len(TASK_IDS)):
        prompt = run(
            [binary, "run", "prompt", run_name, "--project", project],
            cwd=project,
        )
        prompt_bytes_total += len(prompt.stdout)
        step = run(
            [binary, "run", "step", run_name, "--project", project, "--json"],
            cwd=project,
            text=True,
        )
        worker_process_count += 1
        step_report = json.loads(step.stdout)
        terminal_status = str(step_report["status"])
        if terminal_status == "complete":
            break
    if terminal_status != "complete":
        raise BenchmarkError("fixture arm did not complete within four worker starts")

    ledger_result = run(
        [binary, "run", "ledger", run_name, "--project", project, "--json"],
        cwd=project,
        text=True,
    )
    ledger = json.loads(ledger_result.stdout)
    attempts = ledger["attempts"]
    accepted_task_ids = [
        task_id
        for attempt in attempts
        for task_id in attempt["accepted_task_ids"]
    ]
    if accepted_task_ids != TASK_IDS or ledger["accepted_task_count"] != len(TASK_IDS):
        raise BenchmarkError("fixture ledger did not accept the four planned tasks in order")
    if len(attempts) != worker_process_count:
        raise BenchmarkError("attempt ledger and successful fixture process starts differ")

    return {
        "max_packet_tasks": packet_cap,
        "worker_process_count": worker_process_count,
        "process_count_basis": "successful_deterministic_fixture_attempts",
        "attempt_count": len(attempts),
        "accepted_task_ids": accepted_task_ids,
        "prompt_bytes_total": prompt_bytes_total,
        "final_tree_sha256": final_tree_digest(project),
    }


def git_provenance() -> tuple[str | None, bool | None]:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=COMMAND_TIMEOUT_SECONDS,
    )
    dirty = subprocess.run(
        ["git", "status", "--porcelain", "--untracked-files=normal"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
        timeout=COMMAND_TIMEOUT_SECONDS,
    )
    return (
        revision.stdout.strip() if revision.returncode == 0 else None,
        bool(dirty.stdout) if dirty.returncode == 0 else None,
    )


def benchmark(binary: pathlib.Path) -> dict[str, object]:
    binary = binary.resolve(strict=True)
    if not binary.is_file():
        raise BenchmarkError("--binary must name a regular file")
    binary_version = run([binary, "--version"], cwd=ROOT, text=True).stdout.strip()
    source_revision, source_dirty = git_provenance()

    with tempfile.TemporaryDirectory(prefix="codeunlimited-packets-") as directory:
        temporary_root = pathlib.Path(directory)
        arms = {
            "one_task_packets": benchmark_arm(
                binary, temporary_root / "cap-one", packet_cap=1
            ),
            "four_task_packet": benchmark_arm(
                binary, temporary_root / "cap-all", packet_cap=4
            ),
        }

    one_digest = arms["one_task_packets"]["final_tree_sha256"]
    four_digest = arms["four_task_packet"]["final_tree_sha256"]
    if one_digest != four_digest:
        raise BenchmarkError("fixture arms produced different final trees")

    return {
        "schema_version": 1,
        "evidence_scope": "synthetic_offline",
        "fixture_objective": "produce_four_literal_files",
        "arms": arms,
        "equivalent_final_files": True,
        "identical_final_tree_sha256": one_digest,
        "real_token_savings_percent": None,
        "real_token_totals": None,
        "model_request_count": None,
        "native_agent_comparison": "not_run",
        "provider_model_calls": "none",
        "prompt_byte_semantics": (
            "rendered run prompt bytes before each successful fixture step"
        ),
        "provenance": {
            "source_revision": source_revision,
            "source_dirty": source_dirty,
            "binary_version": binary_version,
            "binary_sha256": sha256_file(binary),
            "binary_source_attestation": "not_available_for_caller_supplied_binary",
            "benchmark_script_sha256": sha256_file(pathlib.Path(__file__)),
            "fixture_sha256": sha256_file(FIXTURE),
        },
        "limitations": [
            "Process counts are successful deterministic fixture starts, not model requests.",
            "Prompt bytes are rendered CLI inspection bytes, not tokens or hidden provider traffic.",
            "No competent native-agent arm or provider/model call was run.",
            "The caller-supplied binary digest does not attest its source revision.",
        ],
    }


def render_text(report: dict[str, object]) -> str:
    arms = report["arms"]
    return "\n".join(
        [
            "Offline packet fixture (synthetic evidence)",
            (
                "cap 1: "
                f"{arms['one_task_packets']['worker_process_count']} fixture starts, "
                f"{arms['one_task_packets']['prompt_bytes_total']} prompt bytes"
            ),
            (
                "cap 4: "
                f"{arms['four_task_packet']['worker_process_count']} fixture start, "
                f"{arms['four_task_packet']['prompt_bytes_total']} prompt bytes"
            ),
            "Both arms accepted four tasks and produced the same independently checked files.",
            "No model calls, native-agent comparison, or real token-savings measurement were run.",
        ]
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        report = benchmark(args.binary)
    except (BenchmarkError, FileNotFoundError, json.JSONDecodeError, subprocess.TimeoutExpired) as error:
        print(f"packet benchmark failed: {error}", file=sys.stderr)
        return 1
    if args.json:
        json.dump(report, sys.stdout, sort_keys=True, separators=(",", ":"))
        sys.stdout.write("\n")
    else:
        print(render_text(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
