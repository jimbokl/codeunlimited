#!/usr/bin/env python3
"""Fail fast when release-facing metadata disagrees."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys


SEMVER = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
STRING_ASSIGNMENT = re.compile(
    r'^\s*([A-Za-z0-9_-]+)\s*=\s*("(?:[^"\\]|\\.)*")\s*(?:#.*)?$'
)


def _string_assignment(line: str) -> tuple[str, str] | None:
    match = STRING_ASSIGNMENT.fullmatch(line)
    if match is None:
        return None
    return match.group(1), str(json.loads(match.group(2)))


def table_strings(path: pathlib.Path, table: str) -> dict[str, str]:
    """Read only the quoted string fields used by the release contract.

    This intentionally small reader keeps the checker dependency-free on
    Python 3.10. It is not a general TOML parser.
    """
    current = ""
    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            current = stripped[1:-1].strip()
            continue
        if current == table:
            assignment = _string_assignment(line)
            if assignment is not None:
                key, value = assignment
                values[key] = value
    return values


def lock_package_versions(path: pathlib.Path, name: str) -> set[str]:
    versions: set[str] = set()
    package: dict[str, str] | None = None
    for line in path.read_text(encoding="utf-8").splitlines():
        stripped = line.strip()
        if stripped == "[[package]]":
            if package is not None and package.get("name") == name:
                versions.add(package.get("version", ""))
            package = {}
            continue
        if stripped.startswith("[") and stripped.endswith("]"):
            if package is not None and package.get("name") == name:
                versions.add(package.get("version", ""))
            package = None
            continue
        if package is not None:
            assignment = _string_assignment(line)
            if assignment is not None:
                key, value = assignment
                package[key] = value
    if package is not None and package.get("name") == name:
        versions.add(package.get("version", ""))
    return versions


def check(root: pathlib.Path, expected: str, tag: str | None = None) -> list[str]:
    errors: list[str] = []
    if not SEMVER.fullmatch(expected):
        errors.append(f"expected version is not SemVer x.y.z: {expected}")

    manifest = table_strings(root / "Cargo.toml", "package")
    cargo_version = manifest.get("version", "")
    if cargo_version != expected:
        errors.append(f"Cargo.toml version {cargo_version!r} != expected {expected!r}")
    if manifest.get("rust-version") != "1.82":
        errors.append("Cargo.toml rust-version must be '1.82'")

    lock_versions = lock_package_versions(root / "Cargo.lock", "codeunlimited")
    if lock_versions != {expected}:
        errors.append(
            f"Cargo.lock codeunlimited versions {sorted(lock_versions)!r} != [{expected!r}]"
        )

    project = table_strings(root / "pyproject.toml", "project")
    python_name = project.get("name")
    if python_name != "codeunlimited-reference":
        errors.append(
            "pyproject distribution must be named 'codeunlimited-reference' "
            "so it cannot shadow the Rust CLI"
        )
    scripts = table_strings(root / "pyproject.toml", "project.scripts")
    if scripts != {"codeunlimited-reference": "codeunlimited.cli:main"}:
        errors.append("pyproject must expose only the codeunlimited-reference command")

    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    if f"## {expected}" not in changelog:
        errors.append(f"CHANGELOG.md has no {expected} section")

    runtime_path = root / "docs" / "RUNTIME.md"
    if not runtime_path.is_file():
        errors.append("docs/RUNTIME.md is missing")
    else:
        runtime = runtime_path.read_text(encoding="utf-8").lower()
        for required in (
            "observation plane",
            "execution plane",
            "does not prove realized token savings",
        ):
            if required not in runtime:
                errors.append(f"docs/RUNTIME.md is missing required disclosure: {required}")

    readme = (root / "README.md").read_text(encoding="utf-8")
    if "docs/RUNTIME.md" not in readme:
        errors.append("README.md does not link docs/RUNTIME.md")

    security = (root / "SECURITY.md").read_text(encoding="utf-8").lower()
    for required in ("observation plane", "execution plane", "provider process"):
        if required not in security:
            errors.append(f"SECURITY.md is missing runtime boundary: {required}")

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
