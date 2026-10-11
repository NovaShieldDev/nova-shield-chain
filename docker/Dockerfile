# ─── Aşama 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1.82-slim-bookworm AS builder

# Derleme bağımlılıkları
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Bağımlılıkları önce kopyala (katman önbelleği için)
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Release binary derle
RUN cargo build --release --bin nova-node

# ─── Aşama 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Çalışma zamanı bağımlılıkları
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Nova kullanıcısı oluştur (root ile çalışmaktan kaçın)
RUN groupadd --gid 1000 nova \
    && useradd --uid 1000 --gid nova --shell /bin/bash --create-home nova

# Binary kopyala
COPY --from=builder /build/target/release/nova-node /usr/local/bin/nova-node

# Veri dizinleri
RUN mkdir -p /data/nova/keystore /data/nova/snapshots && \
    chown -R nova:nova /data/nova

# Genesis ve chain spec dosyaları
COPY docker/genesis.json /etc/nova/genesis.json
COPY docker/chains/ /etc/nova/chains/

USER nova
WORKDIR /data/nova

# Portlar:
#   30333 → P2P (libp2p)
#    9944 → RPC (HTTP/WS)
#    9615 → Prometheus metrics
EXPOSE 30333 9944 9615

# Sağlık kontrolü
HEALTHCHECK --interval=30s --timeout=10s --start-period=15s --retries=3 \
    CMD nova-node node-info || exit 1

# Varsayılan: mainnet validator
ENTRYPOINT ["nova-node"]
CMD ["--chain", "mainnet", "--data-dir", "/data/nova"]
