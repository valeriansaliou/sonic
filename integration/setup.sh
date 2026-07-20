#!/usr/bin/env bash
# One-time setup of the Python virtualenv used by download_wikipedia.py.
set -euo pipefail

INTEGRATION_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENV_DIR="${INTEGRATION_DIR}/.venv"

if [[ ! -d "${VENV_DIR}" ]]; then
  python3 -m venv "${VENV_DIR}"
fi

"${VENV_DIR}/bin/pip" install --quiet --upgrade pip
"${VENV_DIR}/bin/pip" install --quiet -r "${INTEGRATION_DIR}/requirements.txt"
echo "Virtualenv ready: ${VENV_DIR}"
