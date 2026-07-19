#!/usr/bin/env bash
# Times a sonic-cli import of a Sonic NDJSON file through the local integration
# cluster's router.
# Usage: ./import.sh <ndjson-file> [collection=wikipedia] [mode=fresh]

set -euo pipefail

INTEGRATION_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${INTEGRATION_DIR}/.." && pwd)"
CLI_BIN="${ROOT_DIR}/target/release/sonic-cli"

ROUTER_ADDR="${ROUTER_ADDR:-127.0.0.1:19100}"
ROUTER_PASSWORD="${ROUTER_PASSWORD:-RouterPassword}"
BATCH_DOCUMENTS="${BATCH_DOCUMENTS:-1000}"
# Buffers this many documents and groups them by bucket before batching, so each network \
#   batch mostly lands on a single router backend; defaults to 10x BATCH_DOCUMENTS in the CLI \
#   itself when left unset here.
GROUP_WINDOW="${GROUP_WINDOW:-}"
CONNECTIONS="${CONNECTIONS:-5}"

FILE="${1:?usage: import.sh <ndjson-file> [collection=wikipedia] [mode=fresh]}"
COLLECTION="${2:-wikipedia}"
MODE="${3:-fresh}"

if [[ ! -x "${CLI_BIN}" ]]; then
  echo "Missing ${CLI_BIN}; run ./cluster.sh build first." >&2
  exit 1
fi

echo "Importing ${FILE} ($(wc -l <"${FILE}" | tr -d ' ') lines) into collection '${COLLECTION}' (mode=${MODE}) via ${ROUTER_ADDR}..."

cmd=(
  "${CLI_BIN}"
  --addr "${ROUTER_ADDR}"
  --password "${ROUTER_PASSWORD}"
  --json
  import
  --file "${FILE}"
  --collection "${COLLECTION}"
  --mode "${MODE}"
  --batch-documents "${BATCH_DOCUMENTS}"
  --connections "${CONNECTIONS}"
)
if [[ -n "${GROUP_WINDOW}" ]]; then
  cmd+=(--group-window "${GROUP_WINDOW}")
fi

TIMEFORMAT='Wall time: %R s (user %U s, sys %S s)'
time "${cmd[@]}"
