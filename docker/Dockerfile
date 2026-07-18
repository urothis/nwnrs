# syntax=docker/dockerfile:1
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818

ARG TARGETOS=linux
ARG TARGETARCH
ARG NWN_VERSION
ARG OCI_CREATED
ARG OCI_REVISION
ARG OCI_SOURCE="https://github.com/urothis/nwnrs"
ARG OCI_VERSION
ARG ASSET_MANIFEST_SHA256
ARG PREPARED_MANIFEST_SHA256

LABEL org.opencontainers.image.title="nwserver" \
      org.opencontainers.image.description="Minimal multi-architecture Neverwinter Nights dedicated server" \
      org.opencontainers.image.source="${OCI_SOURCE}" \
      org.opencontainers.image.revision="${OCI_REVISION}" \
      org.opencontainers.image.created="${OCI_CREATED}" \
      org.opencontainers.image.version="${OCI_VERSION}" \
      org.opencontainers.image.vendor="urothis" \
      io.github.urothis.nwserver.asset-manifest.sha256="${ASSET_MANIFEST_SHA256}" \
      io.github.urothis.nwserver.prepared-manifest.sha256="${PREPARED_MANIFEST_SHA256}"

RUN apt-get update \
    && apt-get --no-install-recommends -y install ca-certificates libc6 libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --gid 0 --home-dir /nwn/home --no-create-home nwn \
    && install -d -o 1000 -g 0 -m 0755 /nwn/data \
    && install -d -o 1000 -g 0 -m 0770 /nwn/home /nwn/run

COPY --chown=1000:0 data/bin/${TARGETOS}-${TARGETARCH}/nwserver /nwn/data/bin/${TARGETOS}-${TARGETARCH}/nwserver
COPY --chown=1000:0 data/data /nwn/data/data
COPY --chown=1000:0 data/lang /nwn/data/lang
COPY --chown=1000:0 data/build-info.json /nwn/data/build-info.json
COPY --chown=1000:0 data/asset-manifest.json /nwn/data/asset-manifest.json
COPY --chown=1000:0 data/SHA256SUMS /nwn/data/SHA256SUMS
COPY --chown=1000:0 scripts/run-server.sh /nwn/run-server.sh
COPY --chown=1000:0 scripts/prep-nwn-ini.awk /nwn/prep-nwn-ini.awk

RUN chmod 0755 "/nwn/data/bin/${TARGETOS}-${TARGETARCH}/nwserver" /nwn/run-server.sh \
    && chmod -R g=u /nwn/home /nwn/run

ENV NWN_EXTRA_ARGS="-userdirectory /nwn/run" \
    NWN_TAIL_LOGS=y \
    NWN_VERSION="${NWN_VERSION}"

VOLUME ["/nwn/home"]
EXPOSE 5121/udp
WORKDIR /nwn/data/bin/${TARGETOS}-${TARGETARCH}
USER 1000:0
ENTRYPOINT ["/bin/bash", "/nwn/run-server.sh"]
