"""Closed-world research kernel, NOT a semantic proof or production authorizer.

Snapshot keys are logical inputs. Their values are never executed or interpreted
as paths. ``complete`` is an explicit fixture assumption, not a learned fact.
"""

import hashlib
import json
from dataclasses import asdict, dataclass, replace
from typing import Mapping


@dataclass(frozen=True)
class Contract:
    id: str
    dependencies: tuple[str, ...]
    upstream: tuple[str, ...] = ()
    complete: bool = False


@dataclass(frozen=True)
class Assessment:
    id: str
    route: str
    changed: tuple[str, ...]
    context: tuple[str, ...]
    context_bytes: int
    reason: str
    receipt: str


@dataclass(frozen=True)
class Witness:
    before: Mapping[str, str]
    after: Mapping[str, str]
    before_result: str
    after_result: str


def _encoded(value) -> bytes:
    try:
        return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")
    except (UnicodeError, TypeError, ValueError) as error:
        raise ValueError("non-serializable research input") from error


def _name(value):
    if not isinstance(value, str) or not 1 <= len(value) <= 256 or any(ord(c) < 32 for c in value):
        raise ValueError("invalid logical input identifier")


def _snapshot(value: Mapping[str, str]) -> dict[str, str]:
    if not isinstance(value, Mapping) or not 1 <= len(value) <= 256:
        raise ValueError("snapshot must contain 1..256 logical inputs")
    copied = dict(value)
    for key, item in copied.items():
        _name(key)
        if not isinstance(item, str):
            raise ValueError("snapshot values must be text")
    if len(_encoded(copied)) > 262144:
        raise ValueError("snapshot exceeds 256 KiB")
    return copied


def _contract(value: Contract):
    if not isinstance(value, Contract) or type(value.complete) is not bool:
        raise ValueError("invalid contract")
    _name(value.id)
    for field in (value.dependencies, value.upstream):
        if not isinstance(field, tuple) or len(field) > 256:
            raise ValueError("contract edges must be bounded immutable tuples")
        for key in field:
            _name(key)
        if len(set(field)) != len(field):
            raise ValueError("duplicate contract edge")
    if not value.dependencies and not value.upstream:
        raise ValueError("an empty dependency contract cannot authorize reuse")


def assess(
    contracts: tuple[Contract, ...],
    baseline: Mapping[str, str],
    current: Mapping[str, str],
    *,
    context_budget_bytes: int = 4096,
) -> dict[str, Assessment]:
    """Select re-use/reconsideration/context under explicit closed-world assumptions."""
    if type(context_budget_bytes) is not int or not 1 <= context_budget_bytes <= 262144:
        raise ValueError("context budget must be 1..262144 bytes")
    if not isinstance(contracts, tuple) or not 1 <= len(contracts) <= 64:
        raise ValueError("graph must contain 1..64 contracts")
    before, after = _snapshot(baseline), _snapshot(current)
    graph: dict[str, Contract] = {}
    for contract in contracts:
        _contract(contract)
        if contract.id in graph:
            raise ValueError("duplicate contract id")
        if not set(contract.dependencies) <= before.keys():
            raise ValueError("baseline lacks a declared dependency")
        graph[contract.id] = contract

    closures: dict[str, tuple[set[str], bool]] = {}
    visiting: set[str] = set()

    def visit(key: str) -> tuple[set[str], bool]:
        if key in visiting:
            raise ValueError("cyclic dependency graph")
        if key not in graph:
            raise ValueError("missing upstream contract")
        if key in closures:
            return closures[key]
        visiting.add(key)
        node = graph[key]
        dependencies, complete = set(node.dependencies), node.complete
        for parent in node.upstream:
            parent_dependencies, parent_complete = visit(parent)
            dependencies.update(parent_dependencies)
            complete = complete and parent_complete
        visiting.remove(key)
        closures[key] = dependencies, complete
        return closures[key]

    for key in graph:
        visit(key)
    # Bind a receipt to the entire input and contract, including irrelevant
    # values. Re-assessment after a change can allow reuse; a stale receipt cannot.
    receipt = hashlib.sha256(_encoded({
        "contracts": [asdict(graph[key]) for key in sorted(graph)],
        "baseline": before, "current": after, "budget": context_budget_bytes,
    })).hexdigest()
    unknown = after.keys() - before.keys()
    results = {}
    for key in sorted(graph):
        dependencies, complete = closures[key]
        missing = dependencies - after.keys()
        changed = tuple(sorted(k for k in dependencies if k not in after or before[k] != after[k]))
        context = tuple(sorted(dependencies & after.keys()))
        context_bytes = len(_encoded({k: after[k] for k in context}))
        if unknown:
            route, reason = "abstain", "unknown_inputs"
        elif not complete:
            route, reason = "abstain", "incomplete_dependency_model"
        elif missing:
            route, reason = "abstain", "missing_inputs"
        elif context_bytes > context_budget_bytes:
            route, reason = "abstain", "context_budget"
        elif changed:
            route, reason = "reconsider", "dependency_changed"
        else:
            route, reason = "reuse", "declared_dependencies_unchanged"
        if context_bytes > context_budget_bytes:
            context, context_bytes = (), 0
        results[key] = Assessment(key, route, changed, context, context_bytes, reason, receipt)
    return results


def refine(contract: Contract, witnesses: tuple[Witness, ...]) -> Contract:
    """Monotone candidate refinement; empirical differences do not certify a guard."""
    _contract(contract)
    if not isinstance(witnesses, tuple) or len(witnesses) > 256:
        raise ValueError("witnesses must be a bounded tuple")
    additions: set[str] = set()
    for witness in witnesses:
        if not isinstance(witness, Witness):
            raise ValueError("invalid witness")
        before, after = _snapshot(witness.before), _snapshot(witness.after)
        if before.keys() != after.keys() or not set(contract.dependencies) <= before.keys():
            raise ValueError("a witness must preserve the known input inventory")
        changed = [key for key in before if before[key] != after[key]]
        if len(changed) != 1:
            raise ValueError("only isolated single-input interventions are supported")
        if not isinstance(witness.before_result, str) or not isinstance(witness.after_result, str):
            raise ValueError("witness outcomes must be text")
        if witness.before_result != witness.after_result:
            additions.add(changed[0])
    additions.difference_update(contract.dependencies)
    if not additions:
        return contract
    # Deliberately requires a separate reviewed promotion even after a fix.
    return replace(contract, dependencies=tuple(sorted(set(contract.dependencies) | additions)), complete=False)


def reuse_is_current(
    assessment: Assessment, contracts: tuple[Contract, ...],
    baseline: Mapping[str, str], current: Mapping[str, str], *, context_budget_bytes: int = 4096,
) -> bool:
    """Recheck a receipt, not permission to execute arbitrary or concurrent code."""
    if not isinstance(assessment, Assessment) or assessment.route != "reuse":
        return False
    try:
        actual = assess(contracts, baseline, current, context_budget_bytes=context_budget_bytes)
    except ValueError:
        return False
    return actual.get(assessment.id) == assessment
