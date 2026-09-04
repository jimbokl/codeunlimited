import json
import pathlib
import subprocess
import sys
import unittest

from scripts import dcr_probe


class ProbeTests(unittest.TestCase):
    def test_probe_matches_frozen_routes_and_checks_actual_reuse_outputs(self):
        report = dcr_probe.run_probe()
        self.assertEqual(report["holdout_cases"], 17)
        self.assertEqual(report["dcr"]["routes"], {"reuse": 7, "reconsider": 6, "abstain": 4})
        self.assertEqual(report["dcr"]["route_mismatches"], 0)
        self.assertEqual(report["dcr"]["incorrect_reuses"], 0)
        self.assertEqual(report["dcr"]["correct_reuses"], 7)

    def test_negative_control_exposes_the_omitted_dependency(self):
        report = dcr_probe.run_probe()
        self.assertEqual(report["unrefined"]["incorrect_reuses"], 2)
        strict = next(row for row in report["cases"] if row["id"] == "strict-tabs")
        self.assertEqual(strict["unrefined_output"], "14")
        self.assertEqual(strict["expected"], "invalid")
        self.assertIsNone(strict["dcr_output"])
        missing = next(row for row in report["cases"] if row["id"] == "missing-config")
        self.assertEqual(missing["unrefined_output"], "12")
        self.assertEqual(missing["dcr_route"], "abstain")
        self.assertIsNone(missing["expected"])

    def test_manual_script_can_solve_supported_cases_without_any_model(self):
        report = dcr_probe.run_probe()
        self.assertEqual(report["manual_script"]["accepted"], 14)
        self.assertEqual(report["manual_script"]["abstained"], 3)
        self.assertEqual(report["manual_script"]["incorrect"], 0)
        self.assertEqual(report["paid_pilot_decision"], "no_go_pending_advantage_over_manual_script")

    def test_counterexample_cannot_self_approve_its_refined_guard(self):
        training = dcr_probe.run_probe()["training"]
        self.assertEqual(training["added_dependencies"], ["config.whitespace"])
        self.assertEqual(training["proposal_route"], "abstain")
        self.assertEqual(training["promotion"], "explicit_fixture_author_assumption")

    def test_report_does_not_convert_local_reuse_into_measured_savings(self):
        report = dcr_probe.run_probe()
        self.assertEqual(report["provider_calls"], 0)
        self.assertIsNone(report["token_savings_percent"])
        self.assertEqual(report["evidence_scope"], "synthetic_offline")
        self.assertEqual(report["native_agent_comparison"], "not_run")
        self.assertEqual(len(report["scenario_sha256"]), 64)
        self.assertEqual(len(report["implementation_sha256"]), 64)
        self.assertNotIn(str(pathlib.Path.home()), json.dumps(report))

    def test_output_is_deterministic(self):
        self.assertEqual(dcr_probe.run_probe(), dcr_probe.run_probe())

    def test_cli_runs_actual_probe_and_emits_json(self):
        result = subprocess.run([sys.executable, "-m", "scripts.dcr_probe", "--json"],
                                capture_output=True, text=True, check=True)
        self.assertEqual(result.stderr, "")
        self.assertEqual(json.loads(result.stdout), dcr_probe.run_probe())

    def test_local_parser_is_not_a_source_interpreter(self):
        self.assertIsNone(dcr_probe.manual_parse({"source": "__import__('os').system('false')"}, "12"))
        self.assertIsNone(dcr_probe.manual_parse({}, "12"))

    def test_probe_runs_with_network_processes_and_file_writes_denied(self):
        code = """
import sys
sys.dont_write_bytecode = True
def deny(event, args):
    if event.startswith(('socket.', 'subprocess.')) or event in ('os.system', 'os.exec', 'os.posix_spawn'):
        raise RuntimeError('external execution forbidden')
    if event == 'open':
        mode, flags = args[1], args[2]
        if (isinstance(mode, str) and any(c in mode for c in 'wax+')) or (isinstance(flags, int) and flags & 3):
            raise RuntimeError('file writes forbidden')
sys.addaudithook(deny)
from scripts.dcr_probe import main
raise SystemExit(main(['--json']))
"""
        result = subprocess.run([sys.executable, "-B", "-c", code], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["provider_calls"], 0)


if __name__ == "__main__":
    unittest.main()
