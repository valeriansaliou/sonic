#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STATE_DIR="${STATE_DIR:-${ROOT_DIR}/.data/demo-moviedb}"
DATASET_DIR="${STATE_DIR}/MovieDB-JSON"
MOVIES_JSON="${DATASET_DIR}/movies.json"
MOVIES_NDJSON="${STATE_DIR}/movies.ndjson"
SONIC_ADDR="${SONIC_ADDR:-127.0.0.1:1491}"
SONIC_PASSWORD="${SONIC_PASSWORD:-SecretPassword}"
LIMIT="${LIMIT:-1000}"
BATCH_DOCUMENTS="${BATCH_DOCUMENTS:-1000}"
QUERY="${QUERY:-star wars}"
RESET="${RESET:-0}"
KEEP_RUNNING="${KEEP_RUNNING:-0}"
SERVER_PID=""
COMPLETED=0

cleanup() {
  if [[ "${KEEP_RUNNING}" == "1" && "${COMPLETED}" == "1" ]]; then
    return
  fi
  if [[ -n "${SERVER_PID}" ]] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}"
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

for command in cargo git python3 unzip zip; do
  if ! command -v "${command}" >/dev/null 2>&1; then
    echo "Missing required command: ${command}" >&2
    exit 1
  fi
done

mkdir -p "${STATE_DIR}"

if [[ ! -d "${DATASET_DIR}/.git" ]]; then
  echo "Downloading MovieDB-JSON..."
  git clone --depth 1 https://github.com/tn3w/MovieDB-JSON.git "${DATASET_DIR}"
fi

if [[ ! -f "${MOVIES_JSON}" ]]; then
  echo "Reconstructing movies.json..."
  (
    cd "${DATASET_DIR}"
    zip -s 0 movies.zip --out "${STATE_DIR}/movies-merged.zip"
  )
  unzip -o "${STATE_DIR}/movies-merged.zip" -d "${DATASET_DIR}"
fi

echo "Converting MovieDB to Sonic NDJSON..."
python3 "${ROOT_DIR}/scripts/convert_moviedb.py" \
  "${MOVIES_JSON}" "${MOVIES_NDJSON}" --limit "${LIMIT}"

if [[ ! -f "${MOVIES_JSON}" ]]; then
  echo "MovieDB archive did not contain movies.json" >&2
  exit 1
fi

if [[ "${RESET}" == "1" ]]; then
  rm -rf "${STATE_DIR}/store"
fi

echo "Building Sonic..."
(
  cd "${ROOT_DIR}"
  CARGO_TARGET_DIR="${ROOT_DIR}/target" \
    cargo build --locked --release --bin sonic --no-default-features -F stemming
  CARGO_TARGET_DIR="${ROOT_DIR}/target" \
    cargo build --locked --release -p sonic_client --bin sonic-cli
)

echo "Starting Sonic on ${SONIC_ADDR}..."
if (echo >/dev/tcp/"${SONIC_ADDR%:*}"/"${SONIC_ADDR##*:}") 2>/dev/null; then
  echo "A Sonic server is already listening on ${SONIC_ADDR}." >&2
  echo "Stop it before running the demo to avoid importing into an old schema." >&2
  exit 1
fi
SONIC_CHANNEL__INET="${SONIC_ADDR}" \
SONIC_CHANNEL__AUTH_PASSWORD="${SONIC_PASSWORD}" \
SONIC_SERVER__LOG_LEVEL="${SONIC_SERVER__LOG_LEVEL:-error}" \
SONIC_STORE__KV__PATH="${STATE_DIR}/store/kv" \
SONIC_STORE__FST__PATH="${STATE_DIR}/store/fst" \
  "${ROOT_DIR}/target/release/sonic" -c "${ROOT_DIR}/config.cfg" \
  >"${STATE_DIR}/sonic.log" 2>&1 &
SERVER_PID=$!

READY=0
for _ in {1..100}; do
  if (echo >/dev/tcp/"${SONIC_ADDR%:*}"/"${SONIC_ADDR##*:}") 2>/dev/null; then
    READY=1
    break
  fi
  if ! kill -0 "${SERVER_PID}" 2>/dev/null; then
    echo "Sonic failed to start; see ${STATE_DIR}/sonic.log" >&2
    exit 1
  fi
  sleep 0.1
done
if [[ "${READY}" != "1" ]]; then
  echo "Sonic did not become ready; see ${STATE_DIR}/sonic.log" >&2
  exit 1
fi

echo "Ingesting MovieDB (LIMIT=${LIMIT}, BATCH_DOCUMENTS=${BATCH_DOCUMENTS})..."
(
  cd "${ROOT_DIR}"
  "${ROOT_DIR}/target/release/sonic-cli" \
    --addr "${SONIC_ADDR}" \
    --password "${SONIC_PASSWORD}" \
    import \
    --file "${MOVIES_NDJSON}" \
    --collection movies \
    --mode fresh \
    --batch-documents "${BATCH_DOCUMENTS}"
  echo "Consolidating typo lexicon..."
  CONSOLIDATE_STARTED="${SECONDS}"
  "${ROOT_DIR}/target/release/sonic-cli" \
    --addr "${SONIC_ADDR}" \
    --password "${SONIC_PASSWORD}" \
    consolidate
  echo "Consolidation completed in $((SECONDS - CONSOLIDATE_STARTED))s"
  "${ROOT_DIR}/target/release/sonic-cli" \
    --addr "${SONIC_ADDR}" \
    --password "${SONIC_PASSWORD}" \
    query \
    --collection movies \
    --bucket default \
    --documents \
    "${QUERY}"
)

echo "Demo data: ${STATE_DIR}"
COMPLETED=1
if [[ "${KEEP_RUNNING}" == "1" ]]; then
  echo "Sonic is still running with PID ${SERVER_PID}"
  echo "Stop it with: kill ${SERVER_PID}"
fi
