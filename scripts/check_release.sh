#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: scripts/check_release.sh <major.minor[.patch]>" >&2
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
exec python3 "$script_dir/check_release.py" --root "$script_dir/.." --expected "$expected"
