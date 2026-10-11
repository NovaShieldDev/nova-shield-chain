# ─── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1.85-slim AS builder

WORKDIR /build

# Bağımlılıklar için cache katmanı
RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev libclang-dev clang \
    && rm -rf /var/lib/apt/lists/*

# Workspace Cargo.toml + tüm crate Cargo.toml'ları kopyala (cache için)
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Release binary derle
RUN cargo build --release -p nova-node

# ─── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Binary kopyala
COPY --from=builder /build/target/release/nova-node /usr/local/bin/nova-node
RUN chmod +x /usr/local/bin/nova-node

# Veri dizini
RUN mkdir -p /data/nova

EXPOSE 30333 9933 9944

HEALTHCHECK --interval=10s --timeout=5s --retries=5 \
    CMD curl -sf http://localhost:9933/health || exit 1

ENTRYPOINT ["nova-node"]
