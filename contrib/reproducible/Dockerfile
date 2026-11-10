# SEDRA: the pinned build environment for reproducible releases (see docs/RELEASE.md).
# The base image is pinned by digest and Debian packages come from a fixed snapshot, so every builder gets the same
# compilers and libraries. Changing either is a change of the release environment: note it in docs/RELEASE.md.
FROM rust:1.99.1-slim-trixie@sha256:f6c4a35d21dbb3c89d7a8d056c193e94fc09037f2b58dd858b09c39a47dd02d7

ARG SNAPSHOT=20261101T000000Z
RUN printf 'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/%s trixie main\n' "$SNAPSHOT" \
        > /etc/apt/sources.list \
    && rm -f /etc/apt/sources.list.d/* \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates git gcc g++ make cmake pkg-config protobuf-compiler libprotobuf-dev clang libclang-dev \
    && rm -rf /var/lib/apt/lists/*

# the source is mounted at /sedra-rusty (build-in-docker.sh); git must accept a checkout owned by another user
RUN git config --system --add safe.directory /sedra-rusty
WORKDIR /sedra-rusty
