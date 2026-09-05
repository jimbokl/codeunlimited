"""Behavioral checks for the synthetic, non-executing DCR research kernel."""

import dataclasses
import unittest

from scripts.dcr_contracts import Contract, Witness, assess, refine, reuse_is_current


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.base = {"source": "parser-v1", "config": "trim", "spec": "integer", "README": "hello"}
        self.graph = (
            Contract("parse", ("source", "config"), complete=True),
            Contract("accept", ("spec",), upstream=("parse",), complete=True),
        )

    def test_unrelated_edit_keeps_reuse_and_excludes_unrelated_context(self):
        result = assess(self.graph, self.base, dict(self.base, README="new docs"))
        self.assertEqual(result["accept"].route, "reuse")
        self.assertEqual(result["accept"].context, ("config", "source", "spec"))

    def test_changed_dependency_invalidates_its_downstream_only(self):
        result = assess(self.graph, self.base, dict(self.base, spec="hexadecimal"))
        self.assertEqual(result["parse"].route, "reuse")
        self.assertEqual(result["accept"].route, "reconsider")
        self.assertEqual(result["accept"].changed, ("spec",))

    def test_upstream_change_reaches_downstream_and_context(self):
        result = assess(self.graph, self.base, dict(self.base, config="strict"))
        self.assertEqual(result["parse"].route, "reconsider")
        self.assertEqual(result["accept"].route, "reconsider")
        self.assertEqual(result["accept"].changed, ("config",))
        self.assertNotIn("README", result["accept"].context)

    def test_new_unknown_input_abstains_globally(self):
        result = assess(self.graph, self.base, dict(self.base, hidden_env="override"))
        self.assertEqual({item.route for item in result.values()}, {"abstain"})

    def test_missing_dependency_abstains_including_downstream(self):
        current = {k: v for k, v in self.base.items() if k != "source"}
        result = assess(self.graph, self.base, current)
        self.assertEqual(result["parse"].route, "abstain")
        self.assertEqual(result["accept"].route, "abstain")

    def test_unknown_dependency_model_is_not_certified_by_upstream_consumer(self):
        graph = (dataclasses.replace(self.graph[0], complete=False), self.graph[1])
        result = assess(graph, self.base, self.base)
        self.assertEqual(result["accept"].route, "abstain")

    def test_default_contract_does_not_assume_complete_dependencies(self):
        result = assess((Contract("x", ("source",)),), self.base, self.base)
        self.assertEqual(result["x"].route, "abstain")

    def test_context_budget_measures_serialized_utf8_and_enforces_boundary(self):
        graph = (Contract("x", ("a",), complete=True),)
        # Compact UTF-8 JSON {"a":"é"} is 10 bytes, not 9 characters.
        enough = assess(graph, {"a": "é"}, {"a": "é"}, context_budget_bytes=10)["x"]
        self.assertEqual(enough.route, "reuse")
        self.assertEqual(enough.context_bytes, 10)
        small = assess(graph, {"a": "é"}, {"a": "é"}, context_budget_bytes=9)["x"]
        self.assertEqual(small.route, "abstain")
        self.assertEqual(small.context, ())

    def test_cycles_duplicates_and_missing_baseline_inputs_are_rejected(self):
        cases = [
            (Contract("a", ("source",), upstream=("b",)), Contract("b", ("config",), upstream=("a",))),
            (self.graph[0], self.graph[0]),
            (Contract("a", ("absent",)),),
            (Contract("a", ("source",), upstream=("absent",)),),
            (Contract("a", ()),),
        ]
        for graph in cases:
            with self.subTest(graph=graph), self.assertRaises(ValueError):
                assess(graph, self.base, self.base)

    def test_order_does_not_change_decision_or_receipt(self):
        first = assess(self.graph, self.base, self.base)
        second = assess(tuple(reversed(self.graph)), dict(reversed(list(self.base.items()))), self.base)
        self.assertEqual(first, second)

    def test_stale_snapshot_or_contract_cannot_authorize_dispatch(self):
        receipt = assess(self.graph, self.base, self.base)["parse"]
        self.assertTrue(reuse_is_current(receipt, self.graph, self.base, self.base))
        self.assertFalse(reuse_is_current(receipt, self.graph, self.base, dict(self.base, README="later")))
        changed = (dataclasses.replace(self.graph[0], dependencies=("source",)), self.graph[1])
        self.assertFalse(reuse_is_current(receipt, changed, self.base, self.base))

    def test_nonreuse_or_forged_receipt_cannot_authorize_dispatch(self):
        current = dict(self.base, config="strict")
        receipt = assess(self.graph, self.base, current)["parse"]
        self.assertFalse(reuse_is_current(receipt, self.graph, self.base, current))
        self.assertFalse(reuse_is_current(dataclasses.replace(receipt, route="reuse"), self.graph, self.base, current))

    def test_nontext_inputs_and_invalid_budget_fail_closed(self):
        for value in (None, 3, {"nested": "secret"}):
            with self.subTest(value=value), self.assertRaises(ValueError):
                assess(self.graph, self.base, dict(self.base, config=value))
        for budget in (0, -1, True, 1.5):
            with self.subTest(budget=budget), self.assertRaises(ValueError):
                assess(self.graph, self.base, self.base, context_budget_bytes=budget)

    def test_counterexample_adds_dependency_but_revokes_completeness(self):
        incomplete = Contract("parse", ("source",), complete=True)
        witness = Witness(self.base, dict(self.base, config="strict"), "12", "invalid")
        result = refine(incomplete, (witness,))
        self.assertEqual(result.dependencies, ("config", "source"))
        self.assertFalse(result.complete)
        self.assertEqual(incomplete.dependencies, ("source",))

    def test_equal_output_does_not_prove_a_dependency_irrelevant(self):
        witness = Witness(self.base, dict(self.base, config="other"), "12", "12")
        result = refine(self.graph[0], (witness,))
        self.assertEqual(result.dependencies, ("source", "config"))

    def test_multikey_or_inventory_counterexample_is_not_causal_evidence(self):
        for current in (dict(self.base, config="strict", source="v2"), dict(self.base, extra="x"), self.base):
            with self.subTest(current=current), self.assertRaises(ValueError):
                refine(self.graph[0], (Witness(self.base, current, "12", "invalid"),))


if __name__ == "__main__":
    unittest.main()
