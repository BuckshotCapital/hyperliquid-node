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
apt-get update
apt-get install -y curl ca-certificates catatonit gnupg2
EOF

# Copy Hyperliquid public key & import it. This is also required by hl-visor to verify downloaded binaries
COPY ./etc/hl-pubkey.asc /root/hl-pubkey.asc
RUN <<-EOF
gpg --import /root/hl-pubkey.asc
rm /root/hl-pubkey.asc
EOF

WORKDIR /hl

ARG NETWORK="Mainnet"

RUN <<-EOF
mkdir -v -p /opt/hl/bin /opt/hl/lib

binary_url=""
sig_url=""
case "${NETWORK}" in
	Mainnet)
		binary_url="https://binaries.hyperliquid.xyz/Mainnet/hl-visor"
		sig_url="${binary_url}.asc"
		;;
	Testnet)
		binary_url="https://binaries.hyperliquid-testnet.xyz/Testnet/hl-visor"
		sig_url="${binary_url}.asc"
		;;
	*)
		echo >&2 "Unsupported network ${NETWORK}"
		exit 1
		;;
esac

echo '{"chain": "'"${NETWORK}"'"}' > /opt/hl/lib/visor.json
ln -svf ../lib/visor.json /opt/hl/bin/visor.json

curl -o /opt/hl/bin/hl-visor "${binary_url}"
curl -o /tmp/hl-visor.asc "${sig_url}"

gpg --verify /tmp/hl-visor.asc /opt/hl/bin/hl-visor
chmod 755 /opt/hl/bin/hl-visor
rm /tmp/hl-visor.asc
EOF

COPY --from=hl-bootstrap-builder /build/hl-bootstrap /usr/local/bin/hl-bootstrap

VOLUME /opt/hl/bin
VOLUME /data
WORKDIR /data

# Hyperliquid loves to store data in HOME
RUN <<-EOF
mkdir -p /data/hl/data
ln -s /data/hl /root/hl
EOF

ENV PATH=/opt/hl/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

ENV HL_BOOTSTRAP_OVERRIDE_GOSSIP_CONFIG_MAX_AGE=15m
ENV HL_BOOTSTRAP_SEED_PEERS_AMOUNT=5
ENV HL_BOOTSTRAP_SEED_PEERS_MAX_LATENCY=80ms
ENV HL_BOOTSTRAP_NETWORK=${NETWORK}

# RPC
EXPOSE 3001/tcp
# P2P
EXPOSE 4000-4010/tcp
ENTRYPOINT ["/usr/bin/catatonit", "--", "hl-bootstrap", "--override-gossip-config-path=/data/override_gossip_config.json", "--"]
CMD ["run-non-validator", "--write-trades", "--write-fills", "--write-order-statuses", "--serve-eth-rpc", "--serve-info", "--disable-output-file-buffering"]
