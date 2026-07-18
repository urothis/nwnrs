# nwserver container image

This directory builds the Neverwinter Nights dedicated server image published
as `ghcr.io/urothis/nwserver`. The image supports `linux/amd64` and
`linux/arm64` from one multi-architecture tag.

The repository never downloads proprietary game assets. An internal asset
process stages an installation and a trusted manifest; this project verifies
that contract, packages a reduced resource view with the current Rust `nwnrs`
CLI, builds the default module from its source-controlled project, and builds
the container.

## Trusted asset contract

Preparation requires both an install root and a JSON manifest. Channel and
version are read only from the manifest, so they cannot drift away from the
asset hashes they describe.

The covered file set is exactly:

- both `bin/linux-x86/nwserver-linux` and
  `bin/linux-arm64/nwserver-linux`;
- every regular file below `data`, `lang/en/data`, and `ovr`.

Symlinks in the covered directories are rejected. Every listed SHA-256 is
verified, duplicate paths are rejected, and the manifest must describe the
covered file set exactly. The two server executables are also checked for the
expected 64-bit little-endian ELF architecture.

Manifest schema 1 has this shape:

```json
{
  "schema": 1,
  "version": "8193.37.17",
  "channel": "stable",
  "files": [
    {
      "path": "bin/linux-x86/nwserver-linux",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    }
  ]
}
```

`channel` must be `stable`, `development`, or `preview`. `databuild.txt` is not
a trusted version source and is deliberately not used. The internal asset
process should emit the manifest as part of staging. For local development,
the equivalent no-download helper is:

```bash
docker/scripts/write-asset-manifest.sh \
  /path/to/Neverwinter\ Nights \
  /path/to/nwserver-assets.json \
  8193.37.17 \
  stable
```

## Preparing and building locally

Build `nwnrs`, then create the ignored `docker/data` context:

```bash
cargo build --release --package nwnrs
docker/scripts/prepare-context.sh \
  /path/to/Neverwinter\ Nights \
  /path/to/nwserver-assets.json \
  docker/data \
  target/release/nwnrs
```

Preparation records the input-manifest SHA-256, the exact `nwnrs` executable
SHA-256, both original server hashes, and a digest of every prepared image
payload. The image exposes these as `/nwn/data/asset-manifest.json`,
`/nwn/data/SHA256SUMS`, and `/nwn/data/build-info.json`.

Build a local platform image with:

```bash
docker buildx build \
  --platform linux/arm64 \
  --build-arg NWN_VERSION=8193.37.17 \
  --load \
  --tag nwserver:local \
  docker
```

The Debian base is pinned by digest. Updating it is an intentional source
change, not an implicit consequence of rebuilding later.

## Publishing

The root `Build and publish nwserver image` GitHub workflow is manually
dispatchable and reusable through `workflow_call`. It must run on an internal
self-hosted runner labeled `nwserver-assets`, with absolute paths to the staged
install and trusted manifest. It builds the repository's current Rust CLI and
does not run Steam, Nim, or any game-asset download action.

Every successful run pushes one `linux/amd64` + `linux/arm64` manifest to GHCR
with these tags:

- the mutable channel, such as `stable`;
- the immutable channel-qualified version, such as `8193.37.17-stable`;
- the unqualified version for stable releases only, such as `8193.37.17`.

The image includes OCI source, revision, creation time, version, vendor,
asset-manifest digest, and prepared-payload digest labels. BuildKit also
publishes provenance and an SBOM. There is no Docker Hub path.

## Running

```bash
docker run --rm \
  --publish 5121:5121/udp \
  --volume nwserver-home:/nwn/home \
  --env NWN_MODULE=nwnrs \
  ghcr.io/urothis/nwserver:stable
```

The default identity is `1000:0`. The writable home and runtime directories
are group-writable by GID 0, so orchestrators can assign another non-root UID:

```bash
docker run --rm --user 10001:0 --volume nwserver-home:/nwn/home \
  ghcr.io/urothis/nwserver:stable
```

A custom UID must retain GID 0, and bind-mounted host directories must grant
that group write access. The server image does not require root at runtime.

Common non-secret settings include `NWN_PORT`, `NWN_SERVERNAME`, `NWN_MODULE`,
`NWN_MAXCLIENTS`, `NWN_PUBLICSERVER`, `NWN_NWSYNCURL`, and `NWN_NWSYNCHASH`.
`NWN_LD_PRELOAD` and `NWN_LD_LIBRARY_PATH` are reserved for the future runtime
integration. `NWN_EXTRA_ARGS` is split on whitespace; pass container command
arguments directly when a value must retain spaces.

Passwords are file-only. Plaintext password environment variables are not
supported. Mount a secret and point the matching variable at it:

```bash
docker run --rm \
  --mount type=bind,source=/secure/admin-password,target=/run/secrets/admin-password,readonly \
  --env NWN_ADMINPASSWORD_FILE=/run/secrets/admin-password \
  ghcr.io/urothis/nwserver:stable
```

The supported variables are `NWN_PLAYERPASSWORD_FILE`, `NWN_DMPASSWORD_FILE`,
and `NWN_ADMINPASSWORD_FILE`. The dedicated server ultimately receives these
values as command arguments, so processes with permission to inspect another
process inside the same container may still see them.

The entrypoint persists generated `cryptographic_secret` and `settings.tml`
atomically to `/nwn/home`, including later updates to `settings.tml`. Legacy
`nwn.ini` and `nwnplayer.ini` files in that volume are imported at startup.

## Module source

`nwnrs` has editable source under the repository-root `module` directory. All
GFF-family resources use the canonical `neverwinter.nim` JSON representation.
Its `nwpkg.lock` preserves the archive layout and source hashes needed for a
reproducible build. Binary MOD files are ignored by Git. If the lock's local
source path exists and still matches, `nwpkg` may reuse that binary
byte-for-byte; otherwise `nwnrs pack` rebuilds the MOD from the module project.
The source project is outside the Docker build context, and only the prepared
`data/mod/nwnrs.mod` is copied into the image.

To verify that source independently:

```bash
cargo run --package nwnrs -- pack --force \
  module /tmp/nwnrs.mod
```

## Validation and early runtime milestone

The Docker pull-request check validates the container scripts independently:

```bash
bash -n docker/scripts/*.sh
```

The module pull-request check owns the Rust packaging tests and verifies that
the source-controlled project builds into a non-empty MOD:

```bash
cargo test --package nwnrs-types gff::json --lib
cargo test --package nwnrs-nwpkg
cargo test --package nwnrs
cargo run --package nwnrs -- pack --force module /tmp/nwnrs.mod
test -s /tmp/nwnrs.mod
```

There is intentionally no timer-based smoke test or guessed health check. The
first `nwnrs-runtime` integration milestone should add a structured readiness
signal from real server state, then wire the container health check and CI
startup validation to that signal.

## Licensing

The container tooling in this directory retains the MIT license from the
original `nwserver` project. The surrounding `nwnrs` repository is
GPL-3.0-only. Neverwinter Nights binaries and game data are not licensed by
this repository and remain subject to their owner's terms; consequently, the
published image does not claim a blanket MIT OCI license.
