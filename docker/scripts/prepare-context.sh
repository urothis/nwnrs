#!/usr/bin/env bash

set -Eeuo pipefail

usage() {
  echo "usage: $0 ASSET_ROOT ASSET_MANIFEST OUTPUT [NWNRS_BIN]" >&2
}

fail() {
  echo "prepare-context: $*" >&2
  exit 1
}

if (( $# < 3 || $# > 4 )); then
  usage
  exit 2
fi

asset_root=$1
asset_manifest=$2
output=$3
nwnrs_bin=${4:-nwnrs}
readonly package_data_version=E1
readonly package_data_compression=zlib

[[ -d "$asset_root" ]] || fail "asset root is not a directory: $asset_root"
[[ -f "$asset_manifest" ]] || fail "asset manifest is not a file: $asset_manifest"
[[ -n "$output" && "$output" != "/" && "$output" != "." ]] || fail "unsafe output path: $output"
[[ ! -e "$output" ]] || fail "output already exists: $output"
command -v jq >/dev/null 2>&1 || fail "jq is required"

if [[ "$nwnrs_bin" == */* ]]; then
  [[ -x "$nwnrs_bin" ]] || fail "nwnrs executable is not executable: $nwnrs_bin"
  nwnrs_bin_dir=$(cd "$(dirname "$nwnrs_bin")" && pwd -P)
  nwnrs_bin="$nwnrs_bin_dir/$(basename "$nwnrs_bin")"
else
  nwnrs_bin=$(command -v "$nwnrs_bin") || fail "nwnrs executable not found in PATH: $nwnrs_bin"
fi

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    fail "no SHA-256 tool is available"
  fi
}

manifest_ok=$(jq -r '
  type == "object" and
  .schema == 1 and
  (.version | type == "string" and test("^[0-9]+\\.[0-9]+\\.[0-9]+([.-][0-9A-Za-z][0-9A-Za-z.-]*)?$")) and
  (.channel == "stable" or .channel == "development" or .channel == "preview") and
  (.files | type == "array" and length > 0) and
  all(.files[];
    type == "object" and
    (.path | type == "string" and test("^[^/][^\\t\\r\\n]*$") and (contains("..") | not)) and
    (.sha256 | type == "string" and test("^[0-9a-f]{64}$"))
  )
' "$asset_manifest") || fail "asset manifest is not valid JSON"
[[ "$manifest_ok" == true ]] || fail "asset manifest does not satisfy schema 1"

version=$(jq -r '.version' "$asset_manifest")
channel=$(jq -r '.channel' "$asset_manifest")

required_files=(
  "bin/linux-x86/nwserver-linux"
  "bin/linux-arm64/nwserver-linux"
  "data/cacert.pem"
  "data/nwn_base.key"
  "lang/en/data/dialog.tlk"
)
for relative_file in "${required_files[@]}"; do
  [[ -f "$asset_root/$relative_file" ]] || fail "required asset is missing: $relative_file"
done
[[ -d "$asset_root/ovr" ]] || fail "required asset directory is missing: ovr"

if find "$asset_root/data" "$asset_root/lang/en/data" "$asset_root/ovr" -type l -print -quit | grep -q .; then
  fail "symlinks are not allowed in manifest-covered asset directories"
fi

validate_elf_machine() {
  local binary=$1
  local expected_machine=$2
  local architecture=$3
  local magic
  local class_and_data
  local machine
  magic=$(od -An -tx1 -N4 "$binary" | tr -d '[:space:]')
  class_and_data=$(od -An -tx1 -j4 -N2 "$binary" | tr -d '[:space:]')
  machine=$(od -An -tx1 -j18 -N2 "$binary" | tr -d '[:space:]')
  [[ "$magic" == 7f454c46 && "$class_and_data" == 0201 && "$machine" == "$expected_machine" ]] \
    || fail "expected a 64-bit little-endian $architecture ELF executable: $binary"
}

validate_elf_machine "$asset_root/bin/linux-x86/nwserver-linux" 3e00 x86-64
validate_elf_machine "$asset_root/bin/linux-arm64/nwserver-linux" b700 AArch64

output_name=$(basename "$output")
output_parent=$(dirname "$output")
mkdir -p "$output_parent"
output_parent=$(cd "$output_parent" && pwd -P)
output="$output_parent/$output_name"
lock="$output.lock"
mkdir "$lock" 2>/dev/null || fail "another preparation owns output lock: $lock"
stage=$(mktemp -d "$output_parent/.nwserver-context.XXXXXX")
scratch=$(mktemp -d "$output_parent/.nwserver-verify.XXXXXX")
cleanup() {
  rm -rf -- "$stage" "$scratch" "$lock"
}
trap cleanup EXIT

(
  cd "$asset_root"
  {
    printf '%s\n' "bin/linux-x86/nwserver-linux" "bin/linux-arm64/nwserver-linux"
    find data lang/en/data ovr -type f -print
  } | LC_ALL=C sort -u > "$scratch/actual-files"
)
jq -r '.files[].path' "$asset_manifest" | LC_ALL=C sort > "$scratch/manifest-files"
if ! cmp -s "$scratch/actual-files" "$scratch/manifest-files"; then
  diff -u "$scratch/manifest-files" "$scratch/actual-files" >&2 || true
  fail "asset manifest file set does not exactly match the consumed asset roots"
fi
if [[ $(wc -l < "$scratch/manifest-files" | tr -d '[:space:]') != $(jq '.files | length' "$asset_manifest") ]]; then
  fail "asset manifest contains duplicate paths"
fi

jq -r '.files[] | [.sha256, .path] | @tsv' "$asset_manifest" > "$scratch/hashes"
while IFS=$'\t' read -r expected_hash relative_file; do
  [[ -f "$asset_root/$relative_file" ]] || fail "manifest asset is missing: $relative_file"
  actual_hash=$(sha256_file "$asset_root/$relative_file")
  [[ "$actual_hash" == "$expected_hash" ]] \
    || fail "SHA-256 mismatch for $relative_file: expected $expected_hash, got $actual_hash"
done < "$scratch/hashes"

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
repo_root=$(cd "$script_dir/../.." && pwd -P)
module_source="$repo_root/module"
[[ -d "$module_source" ]] || fail "module source is missing: $module_source"

mkdir -p \
  "$stage/bin/linux-amd64" \
  "$stage/bin/linux-arm64" \
  "$stage/data/mod" \
  "$stage/lang/en/data" \
  "$stage/.nwnrs-user"

NWN_HOME="$asset_root" "$nwnrs_bin" pack \
  --force \
  --root "$asset_root" \
  --user "$stage/.nwnrs-user" \
  --language english \
  --data-version "$package_data_version" \
  --data-compression "$package_data_compression" \
  nwn_base.key \
  "$stage/data"
rmdir "$stage/.nwnrs-user"

(
  cd "$repo_root"
  "$nwnrs_bin" pack --force "$module_source" "$stage/data/mod/nwnrs.mod"
)

[[ -f "$stage/data/nwn_base.key" ]] || fail "nwnrs did not produce nwn_base.key"
find "$stage/data" -maxdepth 1 -type f -name '*.bif' -print -quit | grep -q . \
  || fail "nwnrs did not produce any packaged BIF files"

install -m 0755 "$asset_root/bin/linux-x86/nwserver-linux" "$stage/bin/linux-amd64/nwserver"
install -m 0755 "$asset_root/bin/linux-arm64/nwserver-linux" "$stage/bin/linux-arm64/nwserver"
install -m 0644 "$asset_root/data/cacert.pem" "$stage/data/cacert.pem"
install -m 0644 "$asset_root/lang/en/data/dialog.tlk" "$stage/lang/en/data/dialog.tlk"
install -m 0644 "$asset_manifest" "$stage/asset-manifest.json"

asset_manifest_sha256=$(sha256_file "$asset_manifest")
nwnrs_sha256=$(sha256_file "$nwnrs_bin")
amd64_sha256=$(sha256_file "$asset_root/bin/linux-x86/nwserver-linux")
arm64_sha256=$(sha256_file "$asset_root/bin/linux-arm64/nwserver-linux")

(
  cd "$stage"
  find bin data lang -type f -print | LC_ALL=C sort | while IFS= read -r relative_file; do
    printf '%s  %s\n' "$(sha256_file "$relative_file")" "$relative_file"
  done
  printf '%s  %s\n' "$(sha256_file asset-manifest.json)" asset-manifest.json
) > "$stage/SHA256SUMS"
prepared_manifest_sha256=$(sha256_file "$stage/SHA256SUMS")

jq -n \
  --arg channel "$channel" \
  --arg version "$version" \
  --arg asset_manifest_sha256 "$asset_manifest_sha256" \
  --arg nwnrs_sha256 "$nwnrs_sha256" \
  --arg prepared_manifest_sha256 "$prepared_manifest_sha256" \
  --arg amd64_sha256 "$amd64_sha256" \
  --arg arm64_sha256 "$arm64_sha256" \
  --arg package_data_version "$package_data_version" \
  --arg package_data_compression "$package_data_compression" \
  '{
    schema: 2,
    channel: $channel,
    version: $version,
    asset_manifest_sha256: $asset_manifest_sha256,
    nwnrs_sha256: $nwnrs_sha256,
    prepared_manifest_sha256: $prepared_manifest_sha256,
    server_asset_sha256: {
      "linux/amd64": $amd64_sha256,
      "linux/arm64": $arm64_sha256
    },
    resource_package: {
      version: $package_data_version,
      compression: $package_data_compression
    }
  }' > "$stage/build-info.json"

[[ ! -e "$output" ]] || fail "output appeared while preparation was running: $output"
mv "$stage" "$output"
rm -rf -- "$scratch" "$lock"
trap - EXIT

echo "Prepared nwserver $version ($channel) context at $output"
