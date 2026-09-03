import pathlib
import subprocess
import sys
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECKER = ROOT / "scripts" / "check_release.py"


class ReleaseCheckerTests(unittest.TestCase):
    def run_checker(self, expected: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(CHECKER), "--root", str(ROOT), "--expected", expected],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_matching_release_metadata_passes(self) -> None:
        result = self.run_checker("1.7.0")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_mismatching_expected_version_fails(self) -> None:
        result = self.run_checker("9.9.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("9.9.9", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
