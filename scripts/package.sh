#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/package.sh <major.minor[.patch]>" >&2
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

bash "$script_dir/check_release.sh" "$expected"
(
  cd "$root"
  cargo package --locked
)

archive="$root/target/package/codeunlimited-${expected}.crate"
if [[ ! -f "$archive" ]]; then
  echo "expected package archive was not created: $archive" >&2
  exit 1
fi
printf '%s\n' "$archive"
