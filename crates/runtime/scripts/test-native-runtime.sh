#!/usr/bin/env bash

set -Eeuo pipefail

readonly repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)
readonly build_root=${CARGO_TARGET_DIR:-$repository/target}
readonly fixture_root="$build_root/runtime-fixture"
readonly host="$fixture_root/nwserver-fixture"
readonly targets="$fixture_root/targets"

case "$(uname -s)" in
  Darwin)
    readonly runtime="$build_root/debug/libnwnrs_runtime_sys.dylib"
    ;;
  Linux)
    readonly runtime="$build_root/debug/libnwnrs_runtime_sys.so"
    ;;
  *)
    echo "native runtime fixture supports only macOS and Linux" >&2
    exit 2
    ;;
esac

mkdir -p "$fixture_root"

cargo build --locked --package nwnrs-runtime-sys --lib
cargo build --locked --package nwnrs
rustc "$repository/crates/runtime/fixtures/host.rs" --edition 2024 -o "$host"
cargo run --quiet --locked --package nwnrs-runtime \
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
grep -Fq ' WARN nwnrs::script: fixture warn message' "$runtime_output"
grep -Fq 'ERROR nwnrs::script: fixture error message' "$runtime_output"
