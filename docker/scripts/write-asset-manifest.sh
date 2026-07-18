#!/usr/bin/env bash

set -Eeuo pipefail

fail() {
  echo "write-asset-manifest: $*" >&2
  exit 1
}

if (( $# != 4 )); then
  echo "usage: $0 ASSET_ROOT OUTPUT VERSION CHANNEL" >&2
  exit 2
fi

asset_root=$1
output=$2
version=$3
channel=$4

[[ -d "$asset_root" ]] || fail "asset root is not a directory: $asset_root"
[[ ! -e "$output" ]] || fail "output already exists: $output"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z][0-9A-Za-z.-]*)?$ ]] \
  || fail "invalid NWN version: $version"
case "$channel" in
  stable|development|preview) ;;
  *) fail "channel must be stable, development, or preview" ;;
esac
command -v jq >/dev/null 2>&1 || fail "jq is required"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    fail "no SHA-256 tool is available"
  fi
}

for relative_file in \
  bin/linux-x86/nwserver-linux \
  bin/linux-arm64/nwserver-linux \
  data/cacert.pem \
  data/nwn_base.key \
  lang/en/data/dialog.tlk; do
  [[ -f "$asset_root/$relative_file" ]] || fail "required asset is missing: $relative_file"
done
[[ -d "$asset_root/ovr" ]] || fail "required asset directory is missing: ovr"

scratch=$(mktemp -d "${TMPDIR:-/tmp}/nwserver-manifest.XXXXXX")
cleanup() {
  rm -rf -- "$scratch"
}
trap cleanup EXIT

(
  cd "$asset_root"
  {
    printf '%s\n' bin/linux-x86/nwserver-linux bin/linux-arm64/nwserver-linux
    find data lang/en/data ovr -type f -print
  } | LC_ALL=C sort -u | while IFS= read -r relative_file; do
    [[ "$relative_file" != *$'\t'* && "$relative_file" != *$'\n'* ]] \
      || fail "asset path contains a tab or newline: $relative_file"
    printf '%s\t%s\n' "$relative_file" "$(sha256_file "$relative_file")"
  done
) > "$scratch/files.tsv"

mkdir -p "$(dirname "$output")"
jq -Rn \
  --arg version "$version" \
  --arg channel "$channel" \
  '[inputs | split("\t") | {path: .[0], sha256: .[1]}] |
   {schema: 1, version: $version, channel: $channel, files: .}' \
  < "$scratch/files.tsv" > "$output"

trap - EXIT
rm -rf -- "$scratch"
echo "Wrote asset manifest to $output"
