#!/usr/bin/env bash

set -Eeuo pipefail

if (( $# != 2 )); then
  echo "usage: $0 UNIFIED_ROOT OUTPUT" >&2
  exit 2
fi

readonly repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)
readonly unified_root=$(cd -- "$1" && pwd -P)
readonly output=$2
readonly unified_commit=$(git -C "$unified_root" rev-parse HEAD)
readonly compiler=${CXX:-c++}
readonly probe=$(mktemp "${TMPDIR:-/tmp}/nwnrs-abi-probe.XXXXXX")
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
  rm -f -- "$probe"
}
trap cleanup EXIT

mkdir -p -- "$(dirname -- "$output")"
"$compiler" \
  -std=c++17 \
  -I"$unified_root/NWNXLib" \
  -I"$unified_root/NWNXLib/API" \
  -DNWNRS_UNIFIED_COMMIT=\"$unified_commit\" \
  -DNWNX_TARGET_NWN_BUILD="$nwn_build" \
  -DNWNX_TARGET_NWN_BUILD_REVISION="$nwn_revision" \
  -DNWNX_TARGET_NWN_BUILD_POSTFIX="$nwn_postfix" \
  "$repository/crates/runtime/abi/abi-probe.cpp" \
  -o "$probe"
"$probe" > "$output"

cargo run --quiet --locked --package nwnrs-runtime \
  --example verify-unified-abi -- \
  "$output" \
  "$repository/crates/runtime/targets"
