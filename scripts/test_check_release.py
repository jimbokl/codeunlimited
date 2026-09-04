import pathlib
import subprocess
import sys
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
CHECKER = ROOT / "scripts" / "check_release.py"
CHECKER_SH = ROOT / "scripts" / "check_release.sh"
PACKAGE_SH = ROOT / "scripts" / "package.sh"
AUDIT_PACKAGE_SH = ROOT / "scripts" / "audit-package.sh"


class ReleaseCheckerTests(unittest.TestCase):
    def run_checker(self, expected: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(CHECKER), "--root", str(ROOT), "--expected", expected],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_matching_release_metadata_passes(self) -> None:
        result = self.run_checker("1.8.0")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_mismatching_expected_version_fails(self) -> None:
        result = self.run_checker("9.9.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("9.9.9", result.stdout + result.stderr)

    def test_shell_wrapper_accepts_minor_release(self) -> None:
        result = subprocess.run(
            ["bash", str(CHECKER_SH), "1.8"],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_package_wrappers_reject_invalid_versions(self) -> None:
        for script in (PACKAGE_SH, AUDIT_PACKAGE_SH):
            with self.subTest(script=script.name):
                result = subprocess.run(
                    ["bash", str(script), "not-a-version"],
                    cwd=ROOT,
                    check=False,
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("major.minor", result.stderr)

    def test_audit_requires_the_exact_packaged_archive(self) -> None:
        result = subprocess.run(
            ["bash", str(AUDIT_PACKAGE_SH), "9.9"],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("codeunlimited-9.9.0.crate", result.stderr)


if __name__ == "__main__":
    unittest.main()
