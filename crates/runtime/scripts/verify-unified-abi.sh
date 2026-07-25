#!/usr/bin/env bash

set -Eeuo pipefail

if (( $# != 2 )); then
  echo "usage: $0 UNIFIED_ROOT OUTPUT" >&2
  exit 2
fi

readonly repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)
readonly unified_root=$(cd -- "$1" && pwd -P)
readonly output=$2
unified_commit=${NWNRS_UNIFIED_COMMIT:-}
if [[ -z $unified_commit ]]; then
  if ! command -v git >/dev/null 2>&1; then
    echo "git is unavailable; set NWNRS_UNIFIED_COMMIT to the exact copied Unified revision" >&2
    exit 1
  fi
  unified_commit=$(git -C "$unified_root" rev-parse HEAD)
elif command -v git >/dev/null 2>&1 && [[ -d $unified_root/.git ]]; then
  checkout_commit=$(git -C "$unified_root" rev-parse HEAD)
  if [[ $checkout_commit != "$unified_commit" ]]; then
    echo "Unified checkout is $checkout_commit, not NWNRS_UNIFIED_COMMIT=$unified_commit" >&2
    exit 1
  fi
fi
if [[ ! $unified_commit =~ ^[0-9a-f]{40}$ ]]; then
  echo "Unified commit must contain exactly 40 lowercase hexadecimal characters" >&2
  exit 1
fi
readonly unified_commit
readonly compiler=${CXX:-c++}
readonly c_compiler=${CC:-cc}
readonly probe=$(mktemp "${TMPDIR:-/tmp}/nwnrs-abi-probe.XXXXXX")
readonly sqlite_object=$(mktemp "${TMPDIR:-/tmp}/nwnrs-abi-sqlite.XXXXXX")
readonly unified_cmake="$unified_root/CMakeLists.txt"

cmake_integer() {
  local name=$1
  local value
  value=$(sed -nE "s/^[[:space:]]*set\\(${name}[[:space:]]+([0-9]+)\\).*$/\\1/p" "$unified_cmake")
  if [[ ! $value =~ ^[0-9]+$ ]]; then
    echo "could not read one integer $name from $unified_cmake" >&2
    exit 1
  fi
  printf '%s\n' "$value"
}

nwn_build=$(cmake_integer TARGET_NWN_BUILD)
nwn_revision=$(cmake_integer TARGET_NWN_BUILD_REVISION)
nwn_postfix=$(cmake_integer TARGET_NWN_BUILD_POSTFIX)
readonly nwn_build nwn_revision nwn_postfix

cleanup() {
  rm -f -- "$probe" "$sqlite_object"
}
trap cleanup EXIT

mkdir -p -- "$(dirname -- "$output")"
"$c_compiler" \
  -I"$unified_root/NWNXLib/External/sqlite3/include" \
  -c "$unified_root/NWNXLib/External/sqlite3/sqlite3.c" \
  -o "$sqlite_object"
"$compiler" \
  -std=c++17 \
  -I"$unified_root/NWNXLib" \
  -I"$unified_root/NWNXLib/API" \
  -I"$unified_root/NWNXLib/External/sqlite3/include" \
  -DNWNRS_UNIFIED_COMMIT=\"$unified_commit\" \
  -DNWNX_TARGET_NWN_BUILD="$nwn_build" \
  -DNWNX_TARGET_NWN_BUILD_REVISION="$nwn_revision" \
  -DNWNX_TARGET_NWN_BUILD_POSTFIX="$nwn_postfix" \
  "$repository/crates/runtime/abi/abi-probe.cpp" \
  "$sqlite_object" \
  -o "$probe"
"$probe" > "$output"

cargo run --quiet --locked --package nwnrs-runtime \
  --example verify-unified-abi -- \
  "$output" \
  "$repository/crates/runtime/targets"
