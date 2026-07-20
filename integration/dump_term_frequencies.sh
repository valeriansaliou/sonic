#!/usr/bin/env bash
# Dumps collection-wide document frequencies from all integration backends.
# Usage: ./dump_term_frequencies.sh <collection> [output.tsv]

set -euo pipefail

INTEGRATION_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${INTEGRATION_DIR}/.." && pwd)"
DATA_DIR="${INTEGRATION_DIR}/data"
COLLECTION="${1:?usage: $0 <collection> [output.tsv]}"
OUTPUT="${2:-${DATA_DIR}/${COLLECTION}-term-frequencies.tsv}"
TERM_DUMP_BIN="${ROOT_DIR}/target/release/sonic-term-dump"
RUST_MIN_VERSION="1.91.0"
CARGO_CMD=(cargo)

rust_version="$(rustc --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || echo 0.0.0)"
if [[ "$(printf '%s\n' "${RUST_MIN_VERSION}" "${rust_version}" | sort -V | head -1)" != "${RUST_MIN_VERSION}" ]]; then
  if command -v rustup >/dev/null 2>&1 && rustup toolchain list | grep -q "^${RUST_MIN_VERSION}"; then
    export RUSTC="$(rustup which --toolchain "${RUST_MIN_VERSION}" rustc)"
    export RUSTDOC="$(rustup which --toolchain "${RUST_MIN_VERSION}" rustdoc)"
    CARGO_CMD=(rustup run "${RUST_MIN_VERSION}" cargo)
  else
    echo "Rust ${RUST_MIN_VERSION}+ is required (found ${rust_version})." >&2
    exit 1
  fi
fi

KV_ROOTS=()
for kv_root in "${DATA_DIR}"/sonic-*/kv; do
  [[ -d "${kv_root}" ]] && KV_ROOTS+=("${kv_root}")
done
if [[ "${#KV_ROOTS[@]}" -eq 0 ]]; then
  echo "No integration backend stores found under ${DATA_DIR}." >&2
  exit 1
fi

mkdir -p "$(dirname "${OUTPUT}")"
RAW_FILE="$(mktemp "${TMPDIR:-/tmp}/sonic-term-frequencies.raw.XXXXXX")"
GROUPED_FILE="$(mktemp "${TMPDIR:-/tmp}/sonic-term-frequencies.grouped.XXXXXX")"
SORTED_FILE="$(mktemp "${TMPDIR:-/tmp}/sonic-term-frequencies.sorted.XXXXXX")"
trap 'rm -f "${RAW_FILE}" "${GROUPED_FILE}" "${SORTED_FILE}"' EXIT

(cd "${ROOT_DIR}" && CARGO_TARGET_DIR="${ROOT_DIR}/target" \
  "${CARGO_CMD[@]}" build --quiet --release -p sonic-core --bin sonic-term-dump)
"${TERM_DUMP_BIN}" "${COLLECTION}" "${KV_ROOTS[@]}" >"${RAW_FILE}"

# Group identical terms before summing their per-bucket frequencies.
LC_ALL=C sort -t $'\t' -k1,1 "${RAW_FILE}" |
  awk -F '\t' 'BEGIN { OFS = FS }
    NR == 1 { term = $1; frequency = $2; next }
    $1 == term { frequency += $2; next }
    { print frequency, term; term = $1; frequency = $2 }
    END { if (NR > 0) print frequency, term }' >"${GROUPED_FILE}"

LC_ALL=C sort -t $'\t' -k1,1nr -k2,2 "${GROUPED_FILE}" >"${SORTED_FILE}"
awk 'BEGIN { print "document_frequency\tterm" } { print }' "${SORTED_FILE}" >"${OUTPUT}"

echo "Wrote $(($(wc -l <"${OUTPUT}") - 1)) terms to ${OUTPUT}"
