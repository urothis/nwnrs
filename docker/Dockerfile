# syntax=docker/dockerfile:1

FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS runtime-builder

ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:${PATH}

RUN apt-get update \
    && apt-get --no-install-recommends -y install \
        build-essential \
        ca-certificates \
        clang \
        curl \
        libclang-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/* \
    && curl --proto '=https' --tlsv1.2 --fail --silent --show-error \
        --output /tmp/rustup-init.sh https://sh.rustup.rs \
    && sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain none \
    && rm /tmp/rustup-init.sh

WORKDIR /src

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --locked --release --package nwnrs \
        --no-default-features --features supervisor \
    && cargo build --locked --release --package nwnrs-runtime-sys \
    && install -D -m 0755 target/release/nwnrs /out/bin/nwnrs \
    && install -D -m 0644 target/release/libnwnrs_runtime_sys.so \
        /out/runtime/libnwnrs_runtime_sys.so

FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS runtime-layout

ARG TARGETOS=linux
ARG TARGETARCH

RUN install -d -o 1000 -g 0 -m 0755 /nwn /nwn/bin /nwn/data /nwn/runtime \
    && install -d -o 1000 -g 0 -m 0770 /nwn/home /nwn/run

FROM gcr.io/distroless/cc-debian12:nonroot@sha256:66aa873a4a14fb164aa01296058efd8253744606d72715e45acface073359faa

ARG TARGETOS=linux
ARG TARGETARCH
ARG NWN_VERSION
ARG OCI_CREATED
ARG OCI_REVISION
ARG OCI_SOURCE="https://github.com/urothis/nwnrs"
ARG OCI_VERSION
ARG ASSET_MANIFEST_SHA256
ARG PREPARED_MANIFEST_SHA256

LABEL org.opencontainers.image.title="nwnrs" \
      org.opencontainers.image.description="Supervised multi-architecture Neverwinter Nights dedicated server" \
      org.opencontainers.image.source="${OCI_SOURCE}" \
      org.opencontainers.image.revision="${OCI_REVISION}" \
      org.opencontainers.image.created="${OCI_CREATED}" \
      org.opencontainers.image.version="${OCI_VERSION}" \
      org.opencontainers.image.vendor="urothis" \
      io.github.urothis.nwnrs.asset-manifest.sha256="${ASSET_MANIFEST_SHA256}" \
      io.github.urothis.nwnrs.prepared-manifest.sha256="${PREPARED_MANIFEST_SHA256}"

COPY docker/passwd /etc/passwd
COPY --from=runtime-layout --chown=1000:0 /nwn /nwn
COPY --from=runtime-builder --chown=1000:0 /out/bin/nwnrs /nwn/bin/nwnrs
COPY --from=runtime-builder --chown=1000:0 /out/runtime/libnwnrs_runtime_sys.so /nwn/runtime/libnwnrs_runtime_sys.so
COPY --chown=1000:0 crates/runtime/targets /nwn/runtime/targets
COPY --chown=1000:0 docker/data/bin/${TARGETOS}-${TARGETARCH}/nwserver /nwn/data/bin/${TARGETOS}-${TARGETARCH}/nwserver
COPY --chown=1000:0 docker/data/data /nwn/data/data
COPY --chown=1000:0 docker/data/lang /nwn/data/lang
COPY --chown=1000:0 docker/data/build-info.json /nwn/data/build-info.json
COPY --chown=1000:0 docker/data/asset-manifest.json /nwn/data/asset-manifest.json
COPY --chown=1000:0 docker/data/SHA256SUMS /nwn/data/SHA256SUMS

ENV NWN_TAIL_LOGS=y \
    NWN_VERSION="${NWN_VERSION}" \
    HOME=/nwn/home

VOLUME ["/nwn/home"]
EXPOSE 5121/udp
WORKDIR /nwn/data/bin/${TARGETOS}-${TARGETARCH}
USER 1000:0
ENTRYPOINT ["/nwn/bin/nwnrs", "run", "--container"]
