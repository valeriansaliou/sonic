#!/usr/bin/env bash
# Manages a local 5-backend sonic-router cluster for integration testing.
# Usage: ./cluster.sh {build|start|stop|restart|status|clean|logs <name>}

set -euo pipefail

INTEGRATION_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${INTEGRATION_DIR}/.." && pwd)"
DATA_DIR="${INTEGRATION_DIR}/data"
LOG_DIR="${DATA_DIR}/logs"
PID_DIR="${DATA_DIR}/pids"
CARGO_TARGET_DIR="${ROOT_DIR}/target"

BACKEND_PASSWORD="SecretPassword"
ROUTER_CONFIG="${INTEGRATION_DIR}/configs/router.cfg"
ROUTER_ADDR="127.0.0.1:19100"
ROUTER_PASSWORD="RouterPassword"
ADMIN_ADDR="127.0.0.1:19101"
ADMIN_PASSWORD="RouterAdminPassword"
BACKEND_PORTS=(19110 19111 19112 19113 19114)
RUST_MIN_VERSION="1.91.0"

# RocksDB tuning for bulk-import benchmarks: the repo's config.cfg defaults \
#   (max_compactions=1, max_flushes=1, ie. only 2 background threads total) are tuned for \
#   steady-state ingestion, not sustained multi-GB bulk loads. Under heavy write pressure, \
#   L0 SST files pile up faster than 2 threads can flush/compact them, and RocksDB's write-stall \
#   kicks in once its L0 file thresholds are hit, which is what a "starts fast then collapses" \
#   import throughput usually means. Override via env if these still aren't enough.
KV_MAX_COMPACTIONS="${KV_MAX_COMPACTIONS:-4}"
KV_MAX_FLUSHES="${KV_MAX_FLUSHES:-2}"
KV_PARALLELISM="${KV_PARALLELISM:-4}"
KV_WRITE_BUFFER_KB="${KV_WRITE_BUFFER_KB:-131072}"
# Use fast active-level compression and denser bottommost compression.
KV_COMPRESS="${KV_COMPRESS:-true}"
# Keep message indexing on general-purpose linguistic tokenization by default.
TOKENIZATION_DETECT_SPECIAL_PATTERNS="${TOKENIZATION_DETECT_SPECIAL_PATTERNS:-false}"
TOKENIZATION_COMPAT_SPLIT_SPECIAL_PATTERNS="${TOKENIZATION_COMPAT_SPLIT_SPECIAL_PATTERNS:-false}"
TOKENIZATION_MAX_TOKEN_LENGTH="${TOKENIZATION_MAX_TOKEN_LENGTH:-128}"

SONIC_BIN="${CARGO_TARGET_DIR}/release/sonic"
ROUTER_BIN="${CARGO_TARGET_DIR}/release/sonic-router"
CLI_BIN="${CARGO_TARGET_DIR}/release/sonic-cli"

CARGO_CMD=(cargo)

resolve_cargo() {
  local version
  version="$(rustc --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || echo 0.0.0)"
  if [[ "$(printf '%s\n' "${RUST_MIN_VERSION}" "${version}" | sort -V | head -1)" == "${RUST_MIN_VERSION}" ]]; then
    CARGO_CMD=(cargo)
    return
  fi
  if command -v rustup >/dev/null 2>&1 && rustup toolchain list | grep -q "^${RUST_MIN_VERSION}"; then
    export RUSTC="$(rustup which --toolchain "${RUST_MIN_VERSION}" rustc)"
    export RUSTDOC="$(rustup which --toolchain "${RUST_MIN_VERSION}" rustdoc)"
    CARGO_CMD=(rustup run "${RUST_MIN_VERSION}" cargo)
    return
  fi
  echo "Rust ${RUST_MIN_VERSION}+ is required (found ${version}) and rustup could not provide it." >&2
  exit 1
}

cmd_build() {
  resolve_cargo
  echo "Building sonic, sonic-router, sonic-cli (release)..."
  ( cd "${ROOT_DIR}" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR}" "${CARGO_CMD[@]}" build --release --bin sonic --no-default-features -F stemming )
  ( cd "${ROOT_DIR}" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR}" "${CARGO_CMD[@]}" build --release -p sonic-router --bin sonic-router )
  ( cd "${ROOT_DIR}" && CARGO_TARGET_DIR="${CARGO_TARGET_DIR}" "${CARGO_CMD[@]}" build --release -p sonic_client --bin sonic-cli )
}

require_binaries() {
  for binary in "${SONIC_BIN}" "${ROUTER_BIN}" "${CLI_BIN}"; do
    if [[ ! -x "${binary}" ]]; then
      echo "Missing binary: ${binary}; run './cluster.sh build' first." >&2
      exit 1
    fi
  done
}

wait_for_port() {
  local host="$1" port="$2" pid="$3" log="$4"
  for _ in $(seq 1 200); do
    if (exec 3<>"/dev/tcp/${host}/${port}") 2>/dev/null; then
      exec 3>&- 3<&-
      return 0
    fi
    if ! kill -0 "${pid}" 2>/dev/null; then
      echo "Process ${pid} exited before listening on ${host}:${port}; see ${log}" >&2
      exit 1
    fi
    sleep 0.1
  done
  echo "Timed out waiting for ${host}:${port}; see ${log}" >&2
  exit 1
}

is_running() {
  local pid_file="$1"
  [[ -f "${pid_file}" ]] && kill -0 "$(cat "${pid_file}")" 2>/dev/null
}

cmd_start() {
  require_binaries
  mkdir -p "${LOG_DIR}" "${PID_DIR}"

  for index in "${!BACKEND_PORTS[@]}"; do
    local port="${BACKEND_PORTS[$index]}"
    local name="sonic-${index}"
    local pid_file="${PID_DIR}/${name}.pid"
    if is_running "${pid_file}"; then
      echo "${name} already running (pid $(cat "${pid_file}"))"
      continue
    fi
    local store_dir="${DATA_DIR}/${name}"
    local profile_path="${LOG_DIR}/${name}.ingest.ndjson"
    mkdir -p "${store_dir}/kv" "${store_dir}/fst"
    : >"${profile_path}"
    echo "Starting ${name} on 127.0.0.1:${port}..."
    SONIC_CHANNEL__INET="127.0.0.1:${port}" \
    SONIC_CHANNEL__AUTH_PASSWORD="${BACKEND_PASSWORD}" \
    SONIC_INGEST_PROFILE_PATH="${profile_path}" \
    SONIC_SERVER__LOG_LEVEL="${SONIC_LOG_LEVEL:-warn}" \
    SONIC_TOKENIZATION__DETECT_SPECIAL_PATTERNS="${TOKENIZATION_DETECT_SPECIAL_PATTERNS}" \
    SONIC_TOKENIZATION__COMPAT_SPLIT_SPECIAL_PATTERNS="${TOKENIZATION_COMPAT_SPLIT_SPECIAL_PATTERNS}" \
    SONIC_TOKENIZATION__MAX_TOKEN_LENGTH="${TOKENIZATION_MAX_TOKEN_LENGTH}" \
    SONIC_STORE__KV__PATH="${store_dir}/kv" \
    SONIC_STORE__FST__PATH="${store_dir}/fst" \
    SONIC_STORE__KV__DATABASE__MAX_COMPACTIONS="${KV_MAX_COMPACTIONS}" \
    SONIC_STORE__KV__DATABASE__MAX_FLUSHES="${KV_MAX_FLUSHES}" \
    SONIC_STORE__KV__DATABASE__PARALLELISM="${KV_PARALLELISM}" \
    SONIC_STORE__KV__DATABASE__WRITE_BUFFER="${KV_WRITE_BUFFER_KB}" \
    SONIC_STORE__KV__DATABASE__COMPRESS="${KV_COMPRESS}" \
      nohup "${SONIC_BIN}" -c "${ROOT_DIR}/config.cfg" >"${LOG_DIR}/${name}.log" 2>&1 </dev/null &
    echo $! >"${pid_file}"
    disown -h %+ 2>/dev/null || true
    wait_for_port "127.0.0.1" "${port}" "$(cat "${pid_file}")" "${LOG_DIR}/${name}.log"
  done

  local router_pid_file="${PID_DIR}/router.pid"
  if is_running "${router_pid_file}"; then
    echo "router already running (pid $(cat "${router_pid_file}"))"
  else
    mkdir -p "${DATA_DIR}/router"
    echo "Starting sonic-router on ${ROUTER_ADDR} (admin on ${ADMIN_ADDR})..."
    # Run from INTEGRATION_DIR (not a subshell) so `directory.path` in router.cfg resolves \
    #   relative to it, while still keeping the background job in the current shell so \
    #   `disown` actually detaches it.
    pushd "${INTEGRATION_DIR}" >/dev/null
    nohup "${ROUTER_BIN}" -c "${ROUTER_CONFIG}" >"${LOG_DIR}/router.log" 2>&1 </dev/null &
    echo $! >"${router_pid_file}"
    disown -h %+ 2>/dev/null || true
    popd >/dev/null
    wait_for_port "127.0.0.1" "19100" "$(cat "${router_pid_file}")" "${LOG_DIR}/router.log"
    wait_for_port "127.0.0.1" "19101" "$(cat "${router_pid_file}")" "${LOG_DIR}/router.log"
  fi

  echo "Cluster ready: router=${ROUTER_ADDR} (password ${ROUTER_PASSWORD}), admin=${ADMIN_ADDR} (password ${ADMIN_PASSWORD})"
}

cmd_stop() {
  for pid_file in "${PID_DIR}"/router.pid "${PID_DIR}"/sonic-*.pid; do
    [[ -f "${pid_file}" ]] || continue
    local name pid
    name="$(basename "${pid_file}" .pid)"
    pid="$(cat "${pid_file}")"
    if kill -0 "${pid}" 2>/dev/null; then
      echo "Stopping ${name} (pid ${pid})..."
      kill "${pid}" 2>/dev/null || true
      for _ in $(seq 1 50); do
        kill -0 "${pid}" 2>/dev/null || break
        sleep 0.1
      done
      kill -9 "${pid}" 2>/dev/null || true
    fi
    rm -f "${pid_file}"
  done
}

cmd_status() {
  for index in "${!BACKEND_PORTS[@]}"; do
    local name="sonic-${index}" port="${BACKEND_PORTS[$index]}"
    if is_running "${PID_DIR}/${name}.pid"; then
      "${CLI_BIN}" --addr "127.0.0.1:${port}" --password "${BACKEND_PASSWORD}" ping >/dev/null 2>&1 \
        && echo "${name}: up (127.0.0.1:${port})" \
        || echo "${name}: pid alive but not answering (127.0.0.1:${port})"
    else
      echo "${name}: down"
    fi
  done
  if is_running "${PID_DIR}/router.pid"; then
    "${CLI_BIN}" --addr "${ROUTER_ADDR}" --password "${ROUTER_PASSWORD}" ping >/dev/null 2>&1 \
      && echo "router: up (${ROUTER_ADDR}, admin ${ADMIN_ADDR})" \
      || echo "router: pid alive but not answering (${ROUTER_ADDR})"
  else
    echo "router: down"
  fi
}

cmd_clean() {
  cmd_stop
  # Only wipe cluster runtime state (RocksDB stores, router directory, logs, pids); \
  #   downloaded/converted datasets (wikipedia*.ndjson, wikipedia-cache/, download.log, ...) \
  #   live under the same DATA_DIR but are expensive to rebuild, so they are left alone.
  echo "Removing cluster state under ${DATA_DIR} (logs, pids, sonic-*, router)..."
  rm -rf "${LOG_DIR}" "${PID_DIR}" "${DATA_DIR}/router"
  for index in "${!BACKEND_PORTS[@]}"; do
    rm -rf "${DATA_DIR}/sonic-${index}"
  done
}

cmd_logs() {
  local name="${1:?usage: cluster.sh logs <sonic-0|sonic-1|...|router>}"
  tail -n 200 -f "${LOG_DIR}/${name}.log"
}

case "${1:-}" in
  build) cmd_build ;;
  start) cmd_start ;;
  stop) cmd_stop ;;
  restart) cmd_stop; cmd_start ;;
  status) cmd_status ;;
  clean) cmd_clean ;;
  logs) cmd_logs "${2:-}" ;;
  *)
    echo "Usage: $0 {build|start|stop|restart|status|clean|logs <name>}" >&2
    exit 1
    ;;
esac
