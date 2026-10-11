#!/usr/bin/env bash
# Nova Shield Chain — Docker tabanlı statik (musl) release build
#
# Kullanım:
#   ./scripts/docker-release.sh                    # mevcut sürümü derle
#   ./scripts/docker-release.sh --version 1.2.0    # sürümü override et
#   ./scripts/docker-release.sh --no-cache         # Docker cache'i atla
#
# Çıktılar (dist/ altında):
#   nova-node-{VERSION}-x86_64-linux-musl           — statik binary
#   nova-node-{VERSION}-x86_64-linux-musl.tar.gz    — arşiv
#   nova-node-{VERSION}-x86_64-linux-musl.sha256    — SHA-256
#   release-{VERSION}.json                          — manifest

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DIST_DIR="${PROJECT_ROOT}/dist"
TARGET="x86_64-unknown-linux-musl"
ARTIFACT_SUFFIX="x86_64-linux-musl"
DOCKER_IMAGE_NAME="nova-shield-builder"
OVERRIDE_VERSION=""
NO_CACHE=""

# ── Argüman ayrıştırma ─────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)  OVERRIDE_VERSION="$2"; shift 2 ;;
        --no-cache) NO_CACHE="--no-cache"; shift ;;
        --help|-h)
            sed -n '2,10p' "$0" | sed 's/^# //'
            exit 0
            ;;
        *) echo "Bilinmeyen argüman: $1" >&2; exit 1 ;;
    esac
done

# ── Sürüm tespiti ─────────────────────────────────────────────────────────────

cd "${PROJECT_ROOT}"

if [[ -n "${OVERRIDE_VERSION}" ]]; then
    VERSION="${OVERRIDE_VERSION}"
else
    VERSION=$(cargo pkgid nova-node 2>/dev/null | sed 's/.*#//' || echo "0.1.0")
fi

ARTIFACT_NAME="nova-node-${VERSION}-${ARTIFACT_SUFFIX}"

echo ""
echo "=== Nova Shield Chain — Docker Statik Release Build ==="
echo "  Sürüm    : ${VERSION}"
echo "  Hedef    : ${TARGET}"
echo "  Artefakt : ${ARTIFACT_NAME}"
echo ""

# ── Dockerfile ────────────────────────────────────────────────────────────────

DOCKERFILE=$(cat <<'DOCKERFILE_EOF'
FROM rust:1.85-slim as builder

# musl araçlarını ve bağımlılıkları kur
RUN apt-get update && apt-get install -y --no-install-recommends \
    musl-tools \
    musl-dev \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# musl target ekle
RUN rustup target add x86_64-unknown-linux-musl

# OpenSSL statik link için
ENV OPENSSL_STATIC=1
ENV OPENSSL_DIR=/usr

WORKDIR /build

# Sadece Cargo.toml/lock dosyaları önce kopyala (layer cache)
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Statik binary derle
RUN cargo build --release --bin nova-node \
    --target x86_64-unknown-linux-musl \
    2>&1

# Runtime image: minimal scratch
FROM scratch as runtime
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/nova-node /nova-node
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
ENTRYPOINT ["/nova-node"]
DOCKERFILE_EOF
)

# Dockerfile'ı geçici dosyaya yaz
DOCKERFILE_PATH=$(mktemp /tmp/nova-Dockerfile-XXXXXX)
echo "${DOCKERFILE}" > "${DOCKERFILE_PATH}"

# ── Docker build ──────────────────────────────────────────────────────────────

echo "Docker builder image oluşturuluyor (${DOCKER_IMAGE_NAME}:builder)..."
docker build ${NO_CACHE} \
    -f "${DOCKERFILE_PATH}" \
    --target builder \
    -t "${DOCKER_IMAGE_NAME}:builder-${VERSION}" \
    "${PROJECT_ROOT}"

# Binary'yi container'dan çıkar
echo "Statik binary kopyalanıyor..."
mkdir -p "${DIST_DIR}"

CONTAINER_ID=$(docker create "${DOCKER_IMAGE_NAME}:builder-${VERSION}" /bin/true 2>/dev/null || \
    docker run --rm -d "${DOCKER_IMAGE_NAME}:builder-${VERSION}" sleep 1)

docker cp "${CONTAINER_ID}:/build/target/x86_64-unknown-linux-musl/release/nova-node" \
    "${DIST_DIR}/${ARTIFACT_NAME}"
docker rm -f "${CONTAINER_ID}" 2>/dev/null || true
rm -f "${DOCKERFILE_PATH}"

chmod +x "${DIST_DIR}/${ARTIFACT_NAME}"

# ── SHA-256 ───────────────────────────────────────────────────────────────────

cd "${DIST_DIR}"
sha256sum "${ARTIFACT_NAME}" > "${ARTIFACT_NAME}.sha256"
SHA256=$(awk '{print $1}' "${ARTIFACT_NAME}.sha256")

# ── Tarball ───────────────────────────────────────────────────────────────────

tar czf "${ARTIFACT_NAME}.tar.gz" "${ARTIFACT_NAME}" "${ARTIFACT_NAME}.sha256"

BINARY_SIZE=$(du -sh "${ARTIFACT_NAME}" | cut -f1)
TARBALL_SIZE=$(du -sh "${ARTIFACT_NAME}.tar.gz" | cut -f1)

cd "${PROJECT_ROOT}"

# ── Release manifest ──────────────────────────────────────────────────────────

BUILD_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

cat > "${DIST_DIR}/release-${VERSION}.json" <<EOF
{
  "name": "nova-node",
  "version": "${VERSION}",
  "protocol_version": 1,
  "build_date": "${BUILD_DATE}",
  "build_type": "static-musl",
  "target": "${TARGET}",
  "artifact": "${ARTIFACT_NAME}",
  "tarball": "${ARTIFACT_NAME}.tar.gz",
  "sha256": "${SHA256}",
  "binary_size": "${BINARY_SIZE}",
  "chain_id_mainnet": 1984,
  "chain_id_testnet": 19840,
  "chain_id_devnet": 198400,
  "token": "NOVA",
  "static_linked": true,
  "files": [
    "${ARTIFACT_NAME}",
    "${ARTIFACT_NAME}.tar.gz",
    "${ARTIFACT_NAME}.sha256"
  ]
}
EOF

echo ""
echo "=== Statik Release Tamamlandı ==="
echo "  Binary  : ${ARTIFACT_NAME} (${BINARY_SIZE})"
echo "  Tarball : ${ARTIFACT_NAME}.tar.gz (${TARBALL_SIZE})"
echo "  SHA-256 : ${SHA256}"
echo ""
echo "Doğrulama:"
echo "  sha256sum --check dist/${ARTIFACT_NAME}.sha256"
echo "  ./dist/${ARTIFACT_NAME} version"
echo "  file dist/${ARTIFACT_NAME}  # 'statically linked' görmeli"
