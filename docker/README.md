# nwnrs container image

This directory builds the multi-architecture Neverwinter Nights: EE dedicated
server image published as `ghcr.io/urothis/nwnrs`. It supports `linux/amd64`
and `linux/arm64`.

The image contains prepared game resources, the NWServer binary,
`nwnrs-runtime-sys`, and the supervisor-only `nwnrs` executable. Its final
stage is a pinned, shell-free Distroless image running as the non-root
`nwserver` account (`1000:0`).

## Run

The preferred interface is the full `nwnrs` CLI on the host:

```bash
nwnrs run --docker \
  --docker-image ghcr.io/urothis/nwnrs:stable \
  --docker-arg pull=always
```

The equivalent direct Docker command is:

```bash
docker run --rm \
  --read-only \
  --cap-drop ALL \
  --security-opt no-new-privileges=true \
  --tmpfs /nwn/run:uid=1000,gid=0,mode=0770 \
  --tmpfs /tmp:uid=1000,gid=0,mode=1777 \
  --publish 5121:5121/udp \
  --volume nwserver-home:/nwn/home \
  ghcr.io/urothis/nwnrs:stable
```

The supervisor prepares the server filesystem, injects the runtime, selects
the matching target pack, follows server logs, forwards signals, and preserves
configuration across restarts. Ctrl-C requests a graceful NWServer shutdown.

## Storage and configuration

| Path | Purpose |
| --- | --- |
| `/nwn/data` | Read-only prepared game data and NWServer binaries |
| `/nwn/runtime` | Read-only runtime library and target packs |
| `/nwn/home` | Persistent modules, saves, vaults, configuration, and crash logs |
| `/nwn/run` | Per-launch writable NWServer user directory |

Keep `/nwn/home` on a named volume or bind mount writable by `1000:0`.
Generated `cryptographic_secret` and `settings.tml` files are copied back
atomically. Existing `nwn.ini` and `nwnplayer.ini` files are imported at
startup.

Common settings are provided through `NWN_*` environment variables, including
`NWN_PORT`, `NWN_SERVERNAME`, `NWN_MODULE`, `NWN_MAXCLIENTS`,
`NWN_PUBLICSERVER`, `NWN_NWSYNCURL`, and `NWN_NWSYNCHASH`. Set
`NWN_TAIL_LOGS=n` to disable server-log following and `NWNRS_COLOR` to `auto`,
`always`, or `never`.

Passwords are accepted only through mounted files. Use
`NWN_PLAYERPASSWORD_FILE`, `NWN_DMPASSWORD_FILE`, or
`NWN_ADMINPASSWORD_FILE`; plaintext password environment variables are not
supported.

## Build locally

The repository does not download proprietary assets. Start with a staged NWN
installation containing both Linux server binaries and the required game data.
Then generate the artifact manifest and prepared build context:

```bash
cargo build --release --package nwnrs

docker/scripts/write-asset-manifest.sh \
  /path/to/Neverwinter\ Nights \
  target/nwserver-assets.json \
  8193.37.17 \
  stable

docker/scripts/prepare-context.sh \
  /path/to/Neverwinter\ Nights \
  target/nwserver-assets.json \
  docker/data \
  target/release/nwnrs
```

Both generated locations are ignored by Git. The manifest is nevertheless
included in the final image for future artifact inspection, together with the
prepared payload checksums and build information:

- `/nwn/data/asset-manifest.json`
- `/nwn/data/SHA256SUMS`
- `/nwn/data/build-info.json`

Build the current platform image from the repository root:

```bash
docker buildx build \
  --platform linux/arm64 \
  --build-arg NWN_VERSION=8193.37.17 \
  --load \
  --tag nwserver:local \
  --file docker/Dockerfile \
  .
```

## Publish

`.github/workflows/docker.yml` publishes a combined AMD64 and ARM64 image to
GHCR. It runs on the internal `nwserver-assets` runner and accepts the staged
install path, NWN version, and release channel.

Each build publishes three tags:

- mutable channel: `stable`, `development`, or `preview`;
- mutable version and channel: `8193.37.17-stable`;
- immutable version, channel, and UTC build time:
  `8193.37.18-preview-20260718T234217Z`.

The timestamp format is `YYYYMMDDTHHMMSSZ`. Published images also include OCI
metadata, provenance, an SBOM, the source asset-manifest digest, and the
prepared-payload digest.
