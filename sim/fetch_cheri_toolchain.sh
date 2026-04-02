#!/usr/bin/env bash
# fetch_cheri_toolchain.sh — Download and verify the CTSRD-CHERI toolchain
#
# Fetches toolchain_coralnpu_v2_cheri.tar.xz from the configured URL,
# verifies its SHA-256 checksum, and extracts to CHERI_TOOLCHAIN_DIR.
#
# Environment variables:
#   CHERI_TOOLCHAIN_URL  — Override download URL
#   CHERI_TOOLCHAIN_DIR  — Override install directory (default: ~/.cheri-toolchain)
#   CHERI_TOOLCHAIN_SHA  — Override expected SHA-256 checksum

set -euo pipefail

# -------------------------------------------------------------------------
# Configuration
# -------------------------------------------------------------------------

TOOLCHAIN_ARCHIVE="toolchain_coralnpu_v2_cheri.tar.xz"

# Default install location
: "${CHERI_TOOLCHAIN_DIR:=${HOME}/.cheri-toolchain}"

# SHA-256 checksum of the toolchain archive.
# Update this when the toolchain version changes.
: "${CHERI_TOOLCHAIN_SHA:=e35882541fb7c11ce2e7b65202b6861336abebafe84d00f056254ba9c67122ce}"

# Download URL
: "${CHERI_TOOLCHAIN_URL:=https://storage.googleapis.com/shodan-public-artifacts/toolchain_coralnpu_v2_cheri.tar.xz}"

# Marker file: records the verified checksum of the installed toolchain
STAMP="${CHERI_TOOLCHAIN_DIR}/.toolchain_sha256"

# -------------------------------------------------------------------------
# Helpers
# -------------------------------------------------------------------------

die()  { echo "ERROR: $*" >&2; exit 1; }
info() { echo ":: $*"; }

check_sha256() {
    local file="$1"
    local expected="$2"
    local actual
    actual=$(sha256sum "$file" | awk '{print $1}')
    if [[ "$actual" != "$expected" ]]; then
        die "checksum mismatch for ${file}:
  expected: ${expected}
  actual:   ${actual}
Delete the file and re-download, or update CHERI_TOOLCHAIN_SHA."
    fi
}

# -------------------------------------------------------------------------
# Check if toolchain is already installed and verified
# -------------------------------------------------------------------------

if [[ -f "$STAMP" ]] && [[ "$(cat "$STAMP")" == "$CHERI_TOOLCHAIN_SHA" ]]; then
    info "CHERI toolchain already installed at ${CHERI_TOOLCHAIN_DIR}"
    info "  SHA-256: ${CHERI_TOOLCHAIN_SHA}"
    # Verify the clang binary exists
    CLANG="${CHERI_TOOLCHAIN_DIR}/bin/clang"
    if [[ -x "$CLANG" ]]; then
        info "  clang:   $($CLANG --version 2>&1 | head -1)"
        exit 0
    else
        info "  WARNING: clang not found at ${CLANG}, re-extracting..."
    fi
fi

# -------------------------------------------------------------------------
# Download if archive not present
# -------------------------------------------------------------------------

ARCHIVE_PATH="${CHERI_TOOLCHAIN_DIR}/${TOOLCHAIN_ARCHIVE}"
mkdir -p "${CHERI_TOOLCHAIN_DIR}"

if [[ ! -f "$ARCHIVE_PATH" ]]; then
    if [[ -z "$CHERI_TOOLCHAIN_URL" ]]; then
        # Check common locations
        for candidate in \
            "${HOME}/${TOOLCHAIN_ARCHIVE}" \
            "/opt/${TOOLCHAIN_ARCHIVE}" \
            "$(pwd)/${TOOLCHAIN_ARCHIVE}" \
            "$(dirname "$0")/${TOOLCHAIN_ARCHIVE}"; do
            if [[ -f "$candidate" ]]; then
                info "Found toolchain archive at: ${candidate}"
                ARCHIVE_PATH="$candidate"
                break
            fi
        done

        if [[ ! -f "$ARCHIVE_PATH" ]]; then
            die "Toolchain archive not found. Either:
  1. Set CHERI_TOOLCHAIN_URL to a download URL, or
  2. Place ${TOOLCHAIN_ARCHIVE} in one of:
     - ${HOME}/
     - /opt/
     - $(pwd)/
     - $(dirname "$0")/"
        fi
    else
        info "Downloading toolchain from ${CHERI_TOOLCHAIN_URL}..."
        if command -v curl &>/dev/null; then
            curl -L -o "$ARCHIVE_PATH" "$CHERI_TOOLCHAIN_URL"
        elif command -v wget &>/dev/null; then
            wget -O "$ARCHIVE_PATH" "$CHERI_TOOLCHAIN_URL"
        else
            die "Neither curl nor wget found. Install one and retry."
        fi
    fi
fi

# -------------------------------------------------------------------------
# Verify checksum
# -------------------------------------------------------------------------

if [[ "$CHERI_TOOLCHAIN_SHA" != "TO_BE_SET_AFTER_FIRST_DOWNLOAD" ]]; then
    info "Verifying SHA-256 checksum..."
    check_sha256 "$ARCHIVE_PATH" "$CHERI_TOOLCHAIN_SHA"
    info "  Checksum OK: ${CHERI_TOOLCHAIN_SHA}"
else
    info "WARNING: No checksum configured. Computing SHA-256 for future use:"
    SHA=$(sha256sum "$ARCHIVE_PATH" | awk '{print $1}')
    info "  CHERI_TOOLCHAIN_SHA=${SHA}"
    info "  Set this in fetch_cheri_toolchain.sh or your environment."
    CHERI_TOOLCHAIN_SHA="$SHA"
fi

# -------------------------------------------------------------------------
# Extract
# -------------------------------------------------------------------------

info "Extracting toolchain to ${CHERI_TOOLCHAIN_DIR}..."
tar -xf "$ARCHIVE_PATH" -C "${CHERI_TOOLCHAIN_DIR}" --strip-components=1

# -------------------------------------------------------------------------
# Verify installation
# -------------------------------------------------------------------------

CLANG="${CHERI_TOOLCHAIN_DIR}/bin/clang"
if [[ ! -x "$CLANG" ]]; then
    # Try to find clang in subdirectories (some archives nest)
    FOUND=$(find "${CHERI_TOOLCHAIN_DIR}" -name clang -type f -executable 2>/dev/null | head -1)
    if [[ -n "$FOUND" ]]; then
        info "Found clang at: ${FOUND}"
        CLANG="$FOUND"
    else
        die "clang not found after extraction. Archive structure may differ from expected."
    fi
fi

info "Installed: $($CLANG --version 2>&1 | head -1)"

# Record verified checksum
echo "$CHERI_TOOLCHAIN_SHA" > "$STAMP"

# -------------------------------------------------------------------------
# Print environment setup
# -------------------------------------------------------------------------

cat <<EOF

CHERI toolchain ready. Add to your environment:

  export CHERI_TOOLCHAIN_DIR=${CHERI_TOOLCHAIN_DIR}
  export PATH=\${CHERI_TOOLCHAIN_DIR}/bin:\${PATH}

Build targets:
  - March: rv32i2p1_m2p0_a2p1_c2p0_xcheri0p0
  - ABI:   il32pc64
  - Sysroot: \${CHERI_TOOLCHAIN_DIR}/riscv32-unknown-none-elfcap/

Example:
  clang --target=riscv32-unknown-none-elfcap \\
        -march=rv32imacxcheri -mabi=il32pc64 \\
        -nostdlib -o test.elf test.c

EOF
