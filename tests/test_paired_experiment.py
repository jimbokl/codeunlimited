import json
import unittest

from scripts import analyze_paired_experiment


def arm(requests: int, input_tokens: int) -> dict[str, int]:
    return {"requests": requests, "input_tokens": input_tokens}


class PairedExperimentTests(unittest.TestCase):
    def test_exact_sign_flip_uses_paired_tasks_as_the_inference_unit(self) -> None:
        payload = {
            "schema_version": 1,
            "pairs": [
                {
                    "task_id": "private-a",
                    "control": arm(2, 100),
                    "treatment": arm(2, 90),
                },
                {
                    "task_id": "private-b",
                    "control": arm(4, 200),
                    "treatment": arm(4, 180),
                },
            ],
        }

        result = analyze_paired_experiment.analyze(payload)

        self.assertEqual(result["pairs"], 2)
        self.assertEqual(result["control_input_tokens"], 300)
        self.assertEqual(result["treatment_input_tokens"], 270)
        self.assertEqual(result["observed_input_delta_tokens"], -30)
        self.assertEqual(result["observed_input_change_percent"], -10.0)
        self.assertAlmostEqual(result["observed_capacity_change_percent"], 100 / 9)
        self.assertEqual(result["treatment_lower_pairs"], 2)
        self.assertEqual(result["exact_paired_sign_flip_p_value"], 0.5)
        self.assertEqual(result["inference_unit"], "paired task")
        self.assertEqual(result["causality"], "observational")

    def test_zero_differences_do_not_inflate_permutation_count(self) -> None:
        payload = {
            "schema_version": 1,
            "pairs": [
                {"task_id": "a", "control": arm(1, 100), "treatment": arm(1, 90)},
                {"task_id": "b", "control": arm(1, 50), "treatment": arm(1, 50)},
            ],
        }

        result = analyze_paired_experiment.analyze(payload)

        self.assertEqual(result["nonzero_pairs_for_inference"], 1)
        self.assertEqual(result["exact_paired_sign_flip_p_value"], 1.0)
        self.assertEqual(result["tied_pairs"], 1)

    def test_task_identifiers_are_not_copied_to_output(self) -> None:
        payload = {
            "schema_version": 1,
            "pairs": [
                {
                    "task_id": "secret-customer-task",
                    "control": arm(1, 10),
                    "treatment": arm(1, 11),
                },
                {"task_id": "safe", "control": arm(1, 20), "treatment": arm(1, 19)},
            ],
        }

        output = json.dumps(analyze_paired_experiment.analyze(payload), sort_keys=True)

        self.assertNotIn("secret-customer-task", output)

    def test_invalid_inputs_are_rejected(self) -> None:
        valid_pair = {"task_id": "a", "control": arm(1, 10), "treatment": arm(1, 9)}
        cases = [
            {},
            {"schema_version": 2, "pairs": [valid_pair, valid_pair]},
            {"schema_version": 1, "pairs": [valid_pair]},
            {"schema_version": 1, "pairs": [valid_pair, valid_pair]},
            {
                "schema_version": 1,
                "pairs": [
                    {"task_id": "a", "control": arm(0, 10), "treatment": arm(1, 9)},
                    {"task_id": "b", "control": arm(1, 10), "treatment": arm(1, 9)},
                ],
            },
            {
                "schema_version": 1,
                "pairs": [
                    {"task_id": "a", "control": arm(True, 10), "treatment": arm(1, 9)},
                    {"task_id": "b", "control": arm(1, 10), "treatment": arm(1, 9)},
                ],
            },
            {
                "schema_version": 1,
                "pairs": [
                    {"task_id": "a", "control": arm(1, -1), "treatment": arm(1, 9)},
                    {"task_id": "b", "control": arm(1, 10), "treatment": arm(1, 9)},
                ],
            },
            {
                "schema_version": 1,
                "pairs": [
                    {"task_id": "a", "control": arm(1, 10)},
                    {"task_id": "b", "control": arm(1, 10), "treatment": arm(1, 9)},
                ],
            },
        ]
        for payload in cases:
            with self.subTest(payload=payload):
                with self.assertRaises(ValueError):
                    analyze_paired_experiment.analyze(payload)


if __name__ == "__main__":
    unittest.main()
