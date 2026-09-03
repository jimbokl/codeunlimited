#!/usr/bin/env python3
"""Fail fast when release-facing metadata disagrees."""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import tomllib


SEMVER = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")


def load_toml(path: pathlib.Path) -> dict:
    with path.open("rb") as stream:
        return tomllib.load(stream)


def check(root: pathlib.Path, expected: str, tag: str | None = None) -> list[str]:
    errors: list[str] = []
    if not SEMVER.fullmatch(expected):
        errors.append(f"expected version is not SemVer x.y.z: {expected}")

    manifest = load_toml(root / "Cargo.toml")
    cargo_version = str(manifest.get("package", {}).get("version", ""))
    if cargo_version != expected:
        errors.append(f"Cargo.toml version {cargo_version!r} != expected {expected!r}")
    if manifest.get("package", {}).get("rust-version") != "1.82":
        errors.append("Cargo.toml rust-version must be '1.82'")

    lock = load_toml(root / "Cargo.lock")
    lock_versions = {
        str(package.get("version", ""))
        for package in lock.get("package", [])
        if package.get("name") == "codeunlimited"
    }
    if lock_versions != {expected}:
        errors.append(
            f"Cargo.lock codeunlimited versions {sorted(lock_versions)!r} != [{expected!r}]"
        )

    pyproject = load_toml(root / "pyproject.toml")
    python_name = pyproject.get("project", {}).get("name")
    if python_name != "codeunlimited-reference":
        errors.append(
            "pyproject distribution must be named 'codeunlimited-reference' "
            "so it cannot shadow the Rust CLI"
        )
    scripts = pyproject.get("project", {}).get("scripts", {})
    if scripts != {"codeunlimited-reference": "codeunlimited.cli:main"}:
        errors.append("pyproject must expose only the codeunlimited-reference command")

    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    if f"## {expected}" not in changelog:
        errors.append(f"CHANGELOG.md has no {expected} section")

    if not (root / "LICENSE").is_file():
        errors.append("LICENSE is missing")

    if tag is not None and tag != f"v{expected}":
        errors.append(f"release tag {tag!r} != 'v{expected}'")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path.cwd())
    parser.add_argument("--expected", required=True)
    parser.add_argument("--tag")
    args = parser.parse_args()

    errors = check(args.root.resolve(), args.expected, args.tag)
    if errors:
        for error in errors:
            print(f"release check failed: {error}", file=sys.stderr)
        return 1
    print(f"release metadata is consistent for {args.expected}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
