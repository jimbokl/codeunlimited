import functools
import hashlib
import http.server
import os
import pathlib
import platform
import subprocess
import tempfile
import threading
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


def asset_name() -> str | None:
    system = platform.system()
    machine = platform.machine().lower()
    if system == "Darwin" and machine in {"arm64", "aarch64"}:
        return "codeunlimited-macos-arm64"
    if system == "Linux" and machine in {"x86_64", "amd64"}:
        return "codeunlimited-linux-x86_64"
    return None


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, fmt: str, *args: object) -> None:
        del fmt, args


@unittest.skipIf(asset_name() is None, "no Unix release asset for this platform")
class UnixInstallerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name)
        self.release = self.root / "release"
        self.destination = self.root / "bin"
        self.release.mkdir()
        self.asset = self.release / str(asset_name())
        self.write_asset("#!/bin/sh\necho 'codeunlimited 1.9.0'\n")
        self.write_checksum()
        handler = functools.partial(QuietHandler, directory=str(self.release))
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def tearDown(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        self.temp.cleanup()

    def write_asset(self, text: str) -> None:
        self.asset.write_text(text, encoding="utf-8")
        self.asset.chmod(0o755)

    def write_checksum(self, value: str | None = None) -> None:
        digest = value or hashlib.sha256(self.asset.read_bytes()).hexdigest()
        pathlib.Path(f"{self.asset}.sha256").write_text(
            f"{digest}  {self.asset.name}\n", encoding="ascii"
        )

    def run_installer(
        self, extra_env: dict[str, str] | None = None
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env["CODEUNLIMITED_DOWNLOAD_BASE_URL"] = (
            f"http://127.0.0.1:{self.server.server_port}"
        )
        env["CODEUNLIMITED_INSTALL_DIR"] = str(self.destination)
        env.update(extra_env or {})
        return subprocess.run(
            ["sh", "install.sh"],
            cwd=ROOT,
            env=env,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )

    def installed_bytes(self) -> bytes:
        return (self.destination / "codeunlimited").read_bytes()

    def install_sentinel(self) -> bytes:
        self.destination.mkdir(parents=True, exist_ok=True)
        sentinel = b"existing verified binary\n"
        (self.destination / "codeunlimited").write_bytes(sentinel)
        return sentinel

    def test_valid_checksum_installs_and_reruns_idempotently(self) -> None:
        first = self.run_installer()
        second = self.run_installer()

        self.assertEqual(first.returncode, 0, first.stderr)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertEqual(self.installed_bytes(), self.asset.read_bytes())
        self.assertIn("codeunlimited 1.9.0", first.stdout)
        self.assertIn("add it to PATH", first.stdout)

    def test_missing_checksum_preserves_existing_binary(self) -> None:
        sentinel = self.install_sentinel()
        pathlib.Path(f"{self.asset}.sha256").unlink()

        result = self.run_installer()

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.installed_bytes(), sentinel)

    def test_malformed_checksum_preserves_existing_binary(self) -> None:
        sentinel = self.install_sentinel()
        self.write_checksum("not-a-sha256")

        result = self.run_installer()

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.installed_bytes(), sentinel)

    def test_mismatching_checksum_preserves_existing_binary(self) -> None:
        sentinel = self.install_sentinel()
        self.write_checksum("0" * 64)

        result = self.run_installer()

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.installed_bytes(), sentinel)

    def test_failed_binary_smoke_test_preserves_existing_binary(self) -> None:
        sentinel = self.install_sentinel()
        self.write_asset("#!/bin/sh\nexit 23\n")
        self.write_checksum()

        result = self.run_installer()

        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.installed_bytes(), sentinel)

    def test_binary_is_not_invoked_after_the_atomic_commit(self) -> None:
        marker = self.root / "smoke-count"
        self.write_asset(
            "#!/bin/sh\n"
            'if [ -e "$CODEUNLIMITED_SMOKE_MARKER" ]; then exit 23; fi\n'
            'touch "$CODEUNLIMITED_SMOKE_MARKER"\n'
            "echo 'codeunlimited 1.9.0'\n"
        )
        self.write_checksum()

        result = self.run_installer(
            {"CODEUNLIMITED_SMOKE_MARKER": str(marker)}
        )

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(self.installed_bytes(), self.asset.read_bytes())
        self.assertIn("codeunlimited 1.9.0", result.stdout)


class PowerShellInstallerStructureTests(unittest.TestCase):
    def test_all_fallible_work_precedes_the_atomic_commit(self) -> None:
        script = (ROOT / "install.ps1").read_text(encoding="utf-8")
        replace = script.index("[IO.File]::Replace")

        self.assertLess(script.index("SetEnvironmentVariable"), replace)
        self.assertLess(script.index("& $download --version"), replace)
        self.assertNotIn("& $exe --version", script)
        self.assertIn("if (-not $committed -and $pathChanged)", script)
        self.assertIn(
            "SetEnvironmentVariable('Path', $originalUserPath, 'User')", script
        )

    def test_native_harness_exercises_path_rollback(self) -> None:
        harness = (ROOT / "tests" / "test_install_ps1.ps1").read_text(
            encoding="utf-8"
        )

        self.assertIn("rollback-bin", harness)
        self.assertIn("User PATH changed after failed replacement", harness)


if __name__ == "__main__":
    unittest.main()
