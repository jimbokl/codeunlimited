#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/audit-package.sh <major.minor[.patch]>" >&2
  exit 2
fi

expected="$1"
if [[ "$expected" =~ ^[0-9]+\.[0-9]+$ ]]; then
  expected="${expected}.0"
elif [[ ! "$expected" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "release version must be major.minor or major.minor.patch: $expected" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
root="$script_dir/.."
archive="$root/target/package/codeunlimited-${expected}.crate"
package_prefix="codeunlimited-${expected}"

if [[ ! -f "$archive" ]]; then
  echo "missing package archive: $archive" >&2
  exit 1
fi

archive_entries="$(tar -tzf "$archive")"
if grep -Eq '(^|/)\.\.(/|$)|^/' <<<"$archive_entries"; then
  echo "package contains an unsafe archive path" >&2
  exit 1
fi
for required in Cargo.toml Cargo.lock LICENSE README.md src/main.rs src/lib.rs src/experiment.rs; do
  if ! grep -Fxq "$package_prefix/$required" <<<"$archive_entries"; then
    echo "package is missing required entry: $required" >&2
    exit 1
  fi
done

for forbidden in .github/ scripts/ docs/superpowers/ codeunlimited/ pyproject.toml; do
  if grep -Fq "$package_prefix/$forbidden" <<<"$archive_entries"; then
    echo "package contains forbidden entry: $forbidden" >&2
    exit 1
  fi
done

audit_parent="${TMPDIR:-/tmp}"
audit_dir="$(mktemp -d "$audit_parent/codeunlimited-package-audit.XXXXXX")"
cleanup() {
  if [[ -n "${audit_dir:-}" && -d "$audit_dir" ]]; then
    rm -rf -- "$audit_dir"
  fi
}
trap cleanup EXIT

tar -xzf "$archive" -C "$audit_dir"
package_dir="$audit_dir/$package_prefix"
if [[ ! -d "$package_dir" ]]; then
  echo "archive did not extract the expected package directory: $package_prefix" >&2
  exit 1
fi

cargo test \
  --manifest-path "$package_dir/Cargo.toml" \
  --all-targets \
  --locked

printf 'audited %s\n' "$archive"
