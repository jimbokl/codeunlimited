#!/usr/bin/env python3
"""Analyze exact task-level control/treatment pairs without pseudoreplication."""

from __future__ import annotations

import argparse
import json
import pathlib
import statistics
import sys
from typing import Iterable


def _integer(value: object, name: str, *, positive: bool = False) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{name} must be an integer")
    if value < 0 or (positive and value == 0):
        qualifier = "positive" if positive else "non-negative"
        raise ValueError(f"{name} must be {qualifier}")
    return value


def _arm(value: object, name: str) -> tuple[int, int]:
    if not isinstance(value, dict) or set(value) != {"requests", "input_tokens"}:
        raise ValueError(f"{name} must contain only requests and input_tokens")
    return (
        _integer(value["requests"], f"{name}.requests", positive=True),
        _integer(value["input_tokens"], f"{name}.input_tokens"),
    )


def _sign_flip_p_value(differences: list[int]) -> float:
    nonzero = [difference for difference in differences if difference]
    if not nonzero:
        return 1.0
    if len(nonzero) > 20:
        raise ValueError("more than 20 nonzero pairs require a pre-registered analysis")
    observed = abs(sum(nonzero))
    permutations = 1 << len(nonzero)
    extreme = 0
    magnitudes = [abs(difference) for difference in nonzero]
    for mask in range(permutations):
        candidate = sum(
            magnitude if mask & (1 << index) else -magnitude
            for index, magnitude in enumerate(magnitudes)
        )
        if abs(candidate) >= observed:
            extreme += 1
    return extreme / permutations


def analyze(payload: dict[str, object]) -> dict[str, object]:
    if not isinstance(payload, dict) or set(payload) != {"schema_version", "pairs"}:
        raise ValueError("payload must contain only schema_version and pairs")
    if payload["schema_version"] != 1:
        raise ValueError("unsupported schema_version")
    pairs = payload["pairs"]
    if not isinstance(pairs, list) or len(pairs) < 2:
        raise ValueError("at least two paired tasks are required")

    seen: set[str] = set()
    control_requests = 0
    treatment_requests = 0
    control_tokens = 0
    treatment_tokens = 0
    control_per_request: list[float] = []
    treatment_per_request: list[float] = []
    differences: list[int] = []

    for index, pair in enumerate(pairs):
        if not isinstance(pair, dict) or set(pair) != {"task_id", "control", "treatment"}:
            raise ValueError(f"pairs[{index}] must contain task_id, control, and treatment")
        task_id = pair["task_id"]
        if not isinstance(task_id, str) or not task_id.strip():
            raise ValueError(f"pairs[{index}].task_id must be a non-empty string")
        if task_id in seen:
            raise ValueError(f"duplicate task_id: {task_id}")
        seen.add(task_id)

        control_request, control_input = _arm(pair["control"], f"pairs[{index}].control")
        treatment_request, treatment_input = _arm(
            pair["treatment"], f"pairs[{index}].treatment"
        )
        control_requests += control_request
        treatment_requests += treatment_request
        control_tokens += control_input
        treatment_tokens += treatment_input
        control_per_request.append(control_input / control_request)
        treatment_per_request.append(treatment_input / treatment_request)
        differences.append(treatment_input - control_input)

    delta = treatment_tokens - control_tokens
    return {
        "schema_version": 1,
        "analysis": "exact paired-task sign-flip",
        "inference_unit": "paired task",
        "causality": "observational",
        "pairs": len(pairs),
        "nonzero_pairs_for_inference": sum(difference != 0 for difference in differences),
        "control_requests": control_requests,
        "treatment_requests": treatment_requests,
        "control_input_tokens": control_tokens,
        "treatment_input_tokens": treatment_tokens,
        "observed_input_delta_tokens": delta,
        "observed_input_change_percent": (
            100.0 * delta / control_tokens if control_tokens else None
        ),
        "observed_capacity_change_percent": (
            100.0 * (control_tokens / treatment_tokens - 1.0)
            if treatment_tokens
            else None
        ),
        "control_median_task_average_tokens_per_request": statistics.median(
            control_per_request
        ),
        "treatment_median_task_average_tokens_per_request": statistics.median(
            treatment_per_request
        ),
        "treatment_lower_pairs": sum(difference < 0 for difference in differences),
        "treatment_higher_pairs": sum(difference > 0 for difference in differences),
        "tied_pairs": sum(difference == 0 for difference in differences),
        "exact_paired_sign_flip_p_value": _sign_flip_p_value(differences),
    }


def render_text(result: dict[str, object]) -> str:
    change = result["observed_input_change_percent"]
    change_text = "n/a" if change is None else f"{float(change):+.1f}%"
    return "\n".join(
        [
            f"paired tasks: {result['pairs']}",
            f"exact observed input: {result['control_input_tokens']:,} control -> {result['treatment_input_tokens']:,} treatment",
            f"observed total change: {result['observed_input_delta_tokens']:+,} ({change_text})",
            "task outcomes (treatment lower/higher/tied): "
            f"{result['treatment_lower_pairs']}/{result['treatment_higher_pairs']}/{result['tied_pairs']}",
            f"exact paired sign-flip p-value: {result['exact_paired_sign_flip_p_value']:.6g}",
            "inference unit: paired task; causality: observational",
        ]
    )


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=pathlib.Path)
    parser.add_argument("--json", action="store_true", dest="json_output")
    args = parser.parse_args(list(argv) if argv is not None else None)
    try:
        payload = json.loads(args.input.read_text(encoding="utf-8"))
        result = analyze(payload)
    except (OSError, json.JSONDecodeError, ValueError) as error:
        print(f"paired experiment failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, sort_keys=True) if args.json_output else render_text(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
