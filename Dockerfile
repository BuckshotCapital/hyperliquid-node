# docker build --load -t localhost/buckshotcapital/hl-node:latest .
ARG mold_version="2.40.2"
ARG rust_version="1.87.0"

FROM rust:${rust_version} AS rust-base

FROM rust:${rust_version} AS hl-bootstrap-builder
RUN    apt-get update \
    && apt-get install -y curl ca-certificates protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

ARG mold_version
RUN curl -L -o /mold.tar.gz https://github.com/rui314/mold/releases/download/v${mold_version}/mold-${mold_version}-$(uname -m)-linux.tar.gz \
    && mkdir -p /opt/mold \
    && tar -C /opt/mold --strip-components=1 -xzf /mold.tar.gz \
    && rm /mold.tar.gz
ENV PATH="/opt/mold/bin:${PATH}"

WORKDIR /build

ENV CARGO_INCREMENTAL="0"
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="gcc"
ENV CFLAGS="-fuse-ld=mold"
ENV RUSTFLAGS="-C link-arg=-fuse-ld=mold"

RUN --mount=source=hl-bootstrap,target=. \
    --mount=type=cache,sharing=locked,target=/usr/local/cargo/registry \
    --mount=type=cache,sharing=locked,from=rust-base,source=/usr/local/rustup,target=/usr/local/rustup \
    cargo fetch --locked

RUN --mount=source=hl-bootstrap,target=. \
    --mount=type=cache,sharing=locked,target=/usr/local/cargo/registry \
    --mount=type=cache,sharing=locked,from=rust-base,source=/usr/local/rustup,target=/usr/local/rustup \
    --mount=type=cache,sharing=locked,target=/target \
    --network=none <<-EOF
CARGO_BUILD_TARGET=""
RUSTFLAGS="${RUSTFLAGS}"

arch="$(uname -m)"
case "${arch}" in
    x86_64)
        CARGO_BUILD_TARGET="x86_64-unknown-linux-gnu"
        ;;
    aarch64)
        CARGO_BUILD_TARGET="aarch64-unknown-linux-gnu"
        ;;
    *) echo "Unsupported architecture: ${arch}" >&2; exit 1 ;;
esac

export CARGO_BUILD_TARGET
export RUSTFLAGS
cargo build --release --target-dir=/target
EOF

RUN --mount=type=cache,sharing=locked,target=/target,ro \
    mkdir -p /build/$(uname -m) && \
    cp /target/$(uname -m)-*/release/hl-bootstrap /build/hl-bootstrap

FROM ubuntu:24.04

SHELL ["/bin/bash", "-euo", "pipefail", "-c"]

RUN <<-EOF
groupadd -r hyperliquid -g 10001
useradd -r -g hyperliquid -u 10001 -d /home/hyperliquid -s /bin/bash hyperliquid

# Create base directory structure
install -d -m 755 -o root -g root /opt/hl
install -d -m 755 -o hyperliquid -g hyperliquid /home/hyperliquid /data /data/hl /data/hl/data /data/snapshots /opt/hl/bin
ln -s /data/hl /home/hyperliquid/hl
EOF

RUN <<-EOF
apt-get update
apt-get install -y curl ca-certificates catatonit gnupg2
EOF

USER hyperliquid:hyperliquid

VOLUME /opt/hl/bin
VOLUME /data
VOLUME /data/snapshots
WORKDIR /data

# Import Hypqliquid public key. This is also required by hl-visor to verify downloaded binaries
RUN --mount=source=.,target=. <<-EOF
gpg --import etc/hl-pubkey.asc
EOF

COPY --from=hl-bootstrap-builder --chown=root:root /build/hl-bootstrap /usr/local/bin/hl-bootstrap

ENV PATH=/opt/hl/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

ENV HL_BOOTSTRAP_VISOR_BINARY_DIRECTORY=/opt/hl/bin
ENV HL_BOOTSTRAP_OVERRIDE_GOSSIP_CONFIG_MAX_AGE=15m
ENV HL_BOOTSTRAP_SEED_PEERS_AMOUNT=5
ENV HL_BOOTSTRAP_SEED_PEERS_MAX_LATENCY=80ms
ENV HL_BOOTSTRAP_NETWORK=Mainnet
ENV HL_BOOTSTRAP_METRICS_LISTEN_ADDRESS=0.0.0.0:2112
ENV HL_BOOTSTRAP_SNAPSHOT_SERVER_LISTEN_ADDRESS=0.0.0.0:2113

# RPC
EXPOSE 3001/tcp
# P2P
EXPOSE 4000-4010/tcp
# Metrics
EXPOSE 2112/tcp
# Snapshot server
EXPOSE 2113/tcp

ENTRYPOINT ["/usr/bin/catatonit", "--", "hl-bootstrap", "--override-gossip-config-path=/data/override_gossip_config.json", "--"]
CMD ["run-non-validator", "--write-trades", "--write-fills", "--write-order-statuses", "--serve-eth-rpc", "--serve-info", "--disable-output-file-buffering"]
