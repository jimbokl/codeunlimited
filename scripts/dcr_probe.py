"""Synthetic DCR falsification probe. No provider calls or target-repo writes."""

import argparse
import hashlib
import json
import pathlib
import re
from collections import Counter
from dataclasses import replace

from scripts.dcr_contracts import Contract, Witness, assess, refine, reuse_is_current

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCENARIOS = ROOT / "tests" / "fixtures" / "dcr" / "scenarios.json"
REQUIRED = {"source", "config.whitespace", "spec.empty", "spec.negative"}


def manual_parse(snapshot, text):
    """Hand-written baseline reads current policy; independent of graph/recipe.

    This is a closed two-parser DSL, not an interpreter for repository source.
    Unknown semantics produce no result. It intentionally needs no LLM.
    """
    if not REQUIRED <= snapshot.keys() or snapshot.keys() - (REQUIRED | {"README"}):
        return None
    if (snapshot["source"] not in ("decimal-v1", "hex-v1")
            or snapshot["config.whitespace"] not in ("trim", "strict")
            or snapshot["spec.empty"] not in ("zero", "error")
            or snapshot["spec.negative"] not in ("allow", "reject")
            or not isinstance(text, str) or len(text) > 2048):
        return None
    value = text.strip() if snapshot["config.whitespace"] == "trim" else text
    if not value:
        return "0" if snapshot["spec.empty"] == "zero" else "empty"
    if value.startswith("-") and snapshot["spec.negative"] == "reject":
        return "invalid"
    pattern, base = (r"-?[0-9]+", 10) if snapshot["source"] == "decimal-v1" else (r"-?[0-9a-fA-F]+", 16)
    return str(int(value, base)) if re.fullmatch(pattern, value) else "invalid"


def _apply_recipe(recipe, text):
    """Execute the fixed baseline recipe, not the current snapshot's policy."""
    strip, empty_zero, negatives, radix = recipe
    value = text.strip() if strip else text
    if not value:
        return "0" if empty_zero else "empty"
    sign = -1 if value.startswith("-") else 1
    digits = value[1:] if sign == -1 else value
    alphabet = "0123456789abcdef"[:radix]
    if not digits or (sign == -1 and not negatives) or any(c.lower() not in alphabet for c in digits):
        return "invalid"
    number = 0
    for digit in digits.lower():
        number = number * radix + alphabet.index(digit)
    return str(sign * number)


def _current(baseline, case):
    snapshot = dict(baseline)
    snapshot.update(case["updates"])
    for key in case.get("remove", []):
        snapshot.pop(key)
    return snapshot


def _outcome(graph, baseline, current, case, recipe):
    budget = case.get("context_budget_bytes", 4096)
    decision = assess(graph, baseline, current, context_budget_bytes=budget)["accept"]
    output = None
    if reuse_is_current(decision, graph, baseline, current, context_budget_bytes=budget):
        output = _apply_recipe(recipe, case["input"])
    return decision, output


def _summary(rows, prefix):
    reuses = [row for row in rows if row[prefix + "_route"] == "reuse"]
    return {
        "routes": dict(sorted(Counter(row[prefix + "_route"] for row in rows).items())),
        "correct_reuses": sum(row[prefix + "_output"] == row["expected"] for row in reuses),
        "incorrect_reuses": sum(row[prefix + "_output"] != row["expected"] for row in reuses),
        "route_mismatches": sum(row[prefix + "_route"] != row["expected_route"] for row in rows),
    }


def run_probe():
    """Return actual fixture results, never token estimates or a live benchmark."""
    scenario_bytes = SCENARIOS.read_bytes()
    scenarios = json.loads(scenario_bytes)
    baseline, training = scenarios["baseline"], scenarios["training"]
    draft = Contract("parse", ("source",), complete=True)
    consumer = Contract("accept", ("spec.empty", "spec.negative"), upstream=("parse",), complete=True)
    training_current = _current(baseline, training)
    candidate = refine(draft, (Witness(baseline, training_current,
                                       training["before_expected"], training["after_expected"]),))
    proposal = assess((candidate, consumer), baseline, training_current)["accept"]
    # Only the fixture author knows this finite DSL's complete dependency set.
    # This explicit assumption is not inferred from the single witness.
    reviewed = replace(candidate, complete=True)
    recipe = (baseline["config.whitespace"] == "trim", baseline["spec.empty"] == "zero",
              baseline["spec.negative"] == "allow", 10)
    rows = []
    for case in scenarios["holdout"]:
        current = _current(baseline, case)
        graph = (replace(reviewed, complete=case.get("complete", True)), consumer)
        unrefined_graph = (replace(draft, complete=case.get("complete", True)), consumer)
        decision, output = _outcome(graph, baseline, current, case, recipe)
        unrefined, unrefined_output = _outcome(unrefined_graph, baseline, current, case, recipe)
        manual = manual_parse(current, case["input"])
        rows.append({
            "id": case["id"], "expected": case["expected"], "expected_route": case["route"],
            "dcr_route": decision.route, "dcr_output": output,
            "context_keys": list(decision.context), "context_bytes": decision.context_bytes,
            "changed_dependencies": list(decision.changed), "reason": decision.reason,
            "unrefined_route": unrefined.route, "unrefined_output": unrefined_output,
            "whole_snapshot_route": "reuse" if current == baseline else "reconsider",
            "manual_output": manual,
        })
    manual_accepted = sum(row["manual_output"] is not None and row["manual_output"] == row["expected"] for row in rows)
    manual_incorrect = sum(row["manual_output"] is not None and row["manual_output"] != row["expected"] for row in rows)
    implementation = hashlib.sha256()
    for filename in ("dcr_contracts.py", "dcr_probe.py"):
        data = (ROOT / "scripts" / filename).read_bytes()
        implementation.update(filename.encode() + b"\0" + data)
    return {
        "schema_version": 1,
        "evidence_scope": "synthetic_offline",
        "scenario_sha256": hashlib.sha256(scenario_bytes).hexdigest(),
        "implementation_sha256": implementation.hexdigest(),
        "provider_calls": 0,
        "token_savings_percent": None,
        "native_agent_comparison": "not_run",
        "holdout_cases": len(rows),
        "training": {
            "id": training["id"],
            "added_dependencies": sorted(set(candidate.dependencies) - set(draft.dependencies)),
            "proposal_route": proposal.route,
            "promotion": "explicit_fixture_author_assumption",
            "oracle_matches_literals": (manual_parse(baseline, training["input"]) == training["before_expected"]
                                        and manual_parse(training_current, training["input"]) == training["after_expected"]),
        },
        "dcr": _summary(rows, "dcr"),
        "unrefined": _summary(rows, "unrefined"),
        "whole_snapshot": {"routes": dict(sorted(Counter(row["whole_snapshot_route"] for row in rows).items())),
                           "scope": "routing-only strawman, not a safety or token comparator"},
        "manual_script": {"accepted": manual_accepted, "incorrect": manual_incorrect,
                          "abstained": sum(row["manual_output"] is None for row in rows)},
        "paid_pilot_decision": "no_go_pending_advantage_over_manual_script",
        "limitations": [
            "No native model execution or measured token savings.",
            "Fixed handcrafted dependency model and small human-authored holdout.",
            "Counterexamples propose dependencies; they do not prove completeness.",
            "Non-reuse means deferred work, not a completed or free solution.",
            "The manual script already covers all supported cases in this closed DSL.",
            "Snapshot assessment is not an atomic filesystem or sandbox boundary.",
        ],
        "cases": rows,
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", dest="as_json")
    args = parser.parse_args(argv)
    report = run_probe()
    if args.as_json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True, indent=2))
    else:
        print(f"Synthetic holdout: {report['holdout_cases']} cases")
        print(f"DCR routes: {report['dcr']['routes']}; incorrect reuse: {report['dcr']['incorrect_reuses']}")
        print(f"Unrefined incorrect reuse: {report['unrefined']['incorrect_reuses']}")
        print(f"Manual script: {report['manual_script']}")
        print("Provider calls: 0; token savings: not measured")
        print(f"Paid pilot: {report['paid_pilot_decision']}")
    return 0 if (report["dcr"]["incorrect_reuses"] == 0 and report["dcr"]["route_mismatches"] == 0
                 and report["manual_script"]["incorrect"] == 0 and report["training"]["oracle_matches_literals"]) else 1


if __name__ == "__main__":
    raise SystemExit(main())
