#!/usr/bin/env bash
# Nova Shield Chain — Release Build Pipeline
#
# Kullanım:
#   ./scripts/release.sh                    # mevcut sürümü derle
#   ./scripts/release.sh --version 1.2.0    # sürümü override et
#   ./scripts/release.sh --target aarch64-unknown-linux-gnu
#
# Çıktılar (dist/ altında):
#   nova-node-{VERSION}-{TARGET}            — sıkıştırılmamış binary
#   nova-node-{VERSION}-{TARGET}.tar.gz     — arşiv
#   nova-node-{VERSION}-{TARGET}.sha256     — SHA-256 checksum
#   release-{VERSION}.json                  — release manifest

set -euo pipefail

# ── Varsayılanlar ──────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DIST_DIR="${PROJECT_ROOT}/dist"

DEFAULT_TARGET="x86_64-unknown-linux-gnu"
TARGET="${DEFAULT_TARGET}"
OVERRIDE_VERSION=""

# ── Argüman ayrıştırma ─────────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            OVERRIDE_VERSION="$2"
            shift 2
            ;;
        --target)
            TARGET="$2"
            shift 2
            ;;
        --help|-h)
            sed -n '2,15p' "$0" | sed 's/^# //'
            exit 0
            ;;
        *)
            echo "Bilinmeyen argüman: $1" >&2
            exit 1
            ;;
    esac
done

# ── Sürüm tespiti ─────────────────────────────────────────────────────────────

cd "${PROJECT_ROOT}"

if [[ -n "${OVERRIDE_VERSION}" ]]; then
    VERSION="${OVERRIDE_VERSION}"
else
    VERSION=$(cargo pkgid nova-node 2>/dev/null | sed 's/.*#//' || echo "0.1.0")
fi

ARTIFACT_NAME="nova-node-${VERSION}-${TARGET}"

echo ""
echo "=== Nova Shield Chain — Release Build ==="
echo "  Sürüm  : ${VERSION}"
echo "  Hedef  : ${TARGET}"
echo "  Artefakt: ${ARTIFACT_NAME}"
echo ""

# ── Bağımlılık kontrolü ────────────────────────────────────────────────────────

if ! command -v cargo &>/dev/null; then
    echo "HATA: cargo bulunamadı. Rust toolchain yüklü olmalı." >&2
    exit 1
fi

if ! command -v sha256sum &>/dev/null; then
    echo "HATA: sha256sum bulunamadı." >&2
    exit 1
fi

# Cross-compile hedefi için rustup target kontrolü
if [[ "${TARGET}" != "${DEFAULT_TARGET}" ]]; then
    if ! rustup target list --installed 2>/dev/null | grep -q "${TARGET}"; then
        echo "Hedef toolchain ekleniyor: ${TARGET}"
        rustup target add "${TARGET}"
    fi
fi

# ── Derleme ────────────────────────────────────────────────────────────────────

echo "Derleniyor (release, target=${TARGET})..."
cargo build --release --bin nova-node --target "${TARGET}"

BINARY_SRC="${PROJECT_ROOT}/target/${TARGET}/release/nova-node"

if [[ ! -f "${BINARY_SRC}" ]]; then
    echo "HATA: Binary üretilmedi: ${BINARY_SRC}" >&2
    exit 1
fi

BINARY_SIZE=$(du -sh "${BINARY_SRC}" | cut -f1)
echo "Binary boyutu: ${BINARY_SIZE}"

# ── Dist dizini hazırla ────────────────────────────────────────────────────────

mkdir -p "${DIST_DIR}"

BINARY_OUT="${DIST_DIR}/${ARTIFACT_NAME}"
TARBALL_OUT="${DIST_DIR}/${ARTIFACT_NAME}.tar.gz"
CHECKSUM_OUT="${DIST_DIR}/${ARTIFACT_NAME}.sha256"
MANIFEST_OUT="${DIST_DIR}/release-${VERSION}.json"

# ── Binary kopyala ─────────────────────────────────────────────────────────────

cp "${BINARY_SRC}" "${BINARY_OUT}"
chmod +x "${BINARY_OUT}"

# ── SHA-256 checksum ───────────────────────────────────────────────────────────

cd "${DIST_DIR}"
sha256sum "${ARTIFACT_NAME}" > "${ARTIFACT_NAME}.sha256"
SHA256=$(awk '{print $1}' "${ARTIFACT_NAME}.sha256")
echo "SHA-256: ${SHA256}"

# ── Tarball ────────────────────────────────────────────────────────────────────

tar czf "${ARTIFACT_NAME}.tar.gz" "${ARTIFACT_NAME}" "${ARTIFACT_NAME}.sha256"
TARBALL_SIZE=$(du -sh "${TARBALL_OUT}" | cut -f1)
echo "Tarball: ${ARTIFACT_NAME}.tar.gz (${TARBALL_SIZE})"

cd "${PROJECT_ROOT}"

# ── Release manifest ───────────────────────────────────────────────────────────

BUILD_DATE=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
PROTOCOL_VERSION=$(grep -o 'NOVA_PROTOCOL_VERSION.*=.*[0-9]' crates/nova-node/src/lib.rs | grep -o '[0-9]*$' || echo "1")

cat > "${MANIFEST_OUT}" <<EOF
{
  "name": "nova-node",
  "version": "${VERSION}",
  "protocol_version": ${PROTOCOL_VERSION},
  "build_date": "${BUILD_DATE}",
  "target": "${TARGET}",
  "artifact": "${ARTIFACT_NAME}",
  "tarball": "${ARTIFACT_NAME}.tar.gz",
  "sha256": "${SHA256}",
  "chain_id_mainnet": 1984,
  "chain_id_testnet": 19840,
  "chain_id_devnet": 198400,
  "token": "NOVA",
  "files": [
    "${ARTIFACT_NAME}",
    "${ARTIFACT_NAME}.tar.gz",
    "${ARTIFACT_NAME}.sha256"
  ]
}
EOF

echo ""
echo "=== Release Tamamlandı ==="
echo "  Dist dizini : ${DIST_DIR}/"
echo "  Binary      : ${ARTIFACT_NAME}"
echo "  Tarball     : ${ARTIFACT_NAME}.tar.gz"
echo "  SHA-256     : ${SHA256}"
echo "  Manifest    : release-${VERSION}.json"
echo ""

# Kısa özet
echo "Doğrulama:"
echo "  sha256sum --check dist/${ARTIFACT_NAME}.sha256"
echo "  ./dist/${ARTIFACT_NAME} version"
