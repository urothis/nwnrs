#!/usr/bin/env bash

set -Eeuo pipefail

readonly repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)
readonly build_root=${CARGO_TARGET_DIR:-$repository/target}
readonly fixture_root="$build_root/runtime-fixture"
readonly host="$fixture_root/nwserver-fixture"
readonly targets="$fixture_root/targets"
readonly administration_object="$fixture_root/administration.o"

case "$(uname -s)" in
  Darwin)
    readonly runtime="$build_root/debug/libnwnrs_runtime_sys.dylib"
    readonly cpp_runtime=-lc++
    ;;
  Linux)
    readonly runtime="$build_root/debug/libnwnrs_runtime_sys.so"
    readonly cpp_runtime=-lstdc++
    ;;
  *)
    echo "native runtime fixture supports only macOS and Linux" >&2
    exit 2
    ;;
esac

mkdir -p "$fixture_root"

cargo build --locked --package nwnrs-runtime-sys --lib
cargo build --locked --package nwnrs
${CXX:-c++} -std=c++17 -Wall -Wextra -Werror -c \
  "$repository/crates/runtime-sys/tests/fixtures/administration.cpp" \
  -o "$administration_object"
rustc "$repository/crates/runtime-sys/tests/fixtures/host.rs" --edition 2024 \
  -C "link-arg=$administration_object" \
  -C "link-arg=$cpp_runtime" \
  -o "$host"
cargo run --quiet --locked --package nwnrs-runtime-sys \
  --example write-fixture-target-pack -- "$host" "$targets"

test -f "$runtime"
readonly runtime_output="$fixture_root/runtime-output.log"
RUST_LOG='warn,nwnrs::launcher=info,nwnrs::runtime=info,nwnrs::script=trace' \
  "$build_root/debug/nwnrs" run \
  --no-tail-logs \
  --runtime "$runtime" \
  --targets "$targets" \
  "$host" > "$runtime_output" 2>&1

grep -Fq 'TRACE nwnrs::script: fixture trace message' "$runtime_output"
grep -Fq 'DEBUG nwnrs::script: fixture debug message' "$runtime_output"
grep -Fq ' INFO nwnrs::script: fixture info message' "$runtime_output"
grep -Fq ' INFO nwnrs::script: fixture multiline first' "$runtime_output"
grep -Fq ' INFO nwnrs::script: fixture multiline second' "$runtime_output"
grep -Fq ' INFO nwnrs::script: fixture multiline third' "$runtime_output"
grep -Fq ' WARN nwnrs::script: fixture warn message' "$runtime_output"
grep -Fq 'ERROR nwnrs::script: fixture error message' "$runtime_output"
