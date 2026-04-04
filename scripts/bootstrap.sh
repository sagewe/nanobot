#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "missing required command: ${cmd}" >&2
    exit 1
  fi
}

require_cmd cargo
require_cmd node
require_cmd npm

echo "cargo: $(cargo --version)"
echo "node: $(node --version)"
echo "npm: $(npm --version)"

cd "${REPO_ROOT}"

cargo fetch
(
  cd frontend
  npm ci
)

cat <<'EOF'
bootstrap complete
next steps:
  mise exec -- just test
  mise exec -- just gateway
EOF
