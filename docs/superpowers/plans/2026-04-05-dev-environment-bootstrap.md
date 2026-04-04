# Dev Environment Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a reproducible local developer setup flow using `mise.toml`, `justfile`, and `scripts/bootstrap.sh` without creating runtime config.

**Architecture:** Keep responsibilities separated by layer: `mise.toml` owns tool versions, `justfile` owns the small top-level command surface, and `scripts/bootstrap.sh` owns shell-specific prerequisite checks and dependency installation. Because the repository does not have an existing automated harness for validating root-level dev-tooling files, use command-level red/green smoke checks instead of introducing a new test framework.

**Tech Stack:** Rust/Cargo, Node/npm, `mise`, `just`, POSIX shell via Bash, Markdown docs.

---

## File Map

### New Files

- Create: `<repo-root>/mise.toml`
  - Declares the canonical local development tool versions for Node and Rust.
- Create: `<repo-root>/justfile`
  - Exposes the top-level developer commands and delegates shell logic to scripts.
- Create: `<repo-root>/scripts/bootstrap.sh`
  - Performs prerequisite checks, version reporting, `cargo fetch`, and `npm ci`.

### Existing Files

- Modify: `<repo-root>/README.md`
  - Documents the preferred local setup and development flow.

### Reference Spec

- Read: `<repo-root>/docs/superpowers/specs/2026-04-05-dev-environment-bootstrap-design.md`

## Task 1: Add Version Management and Root Command Entry Points

**Files:**
- Create: `<repo-root>/mise.toml`
- Create: `<repo-root>/justfile`

- [ ] **Step 1: Run the red check for missing root tooling files**

Run: `test -f mise.toml; just --list`
Expected:
- `test -f mise.toml` exits non-zero because the file does not exist yet.
- `just --list` exits non-zero because there is no `justfile` in the repository root yet.

- [ ] **Step 2: Create `mise.toml` with the initial tool declarations**

```toml
[tools]
node = "22"
rust = "stable"
```

- [ ] **Step 3: Create the initial `justfile` with a small command surface**

```just
set shell := ["bash", "-euo", "pipefail", "-c"]

bootstrap:
    ./scripts/bootstrap.sh

test:
    cargo test
    cd frontend && npm test -- --run

frontend-test:
    cd frontend && npm test -- --run

build:
    cargo build

gateway:
    cargo run --release -- gateway
```

Implementation notes:
- keep recipes thin; do not inline bootstrap shell logic here
- use the repository root as the only execution context

- [ ] **Step 4: Run the green check for root command discovery**

Run: `just --list`
Expected: PASS and output entries for `bootstrap`, `test`, `frontend-test`, `build`, and `gateway`.

- [ ] **Step 5: Commit the tooling entry-point files**

```bash
git add mise.toml justfile
git commit -m "chore: add dev tooling entry points"
```

## Task 2: Add the Bootstrap Script and Wire the Bootstrap Flow

**Files:**
- Create: `<repo-root>/scripts/bootstrap.sh`
- Modify: `<repo-root>/justfile`

- [ ] **Step 1: Run the red check for the missing bootstrap implementation**

Run: `just bootstrap`
Expected: FAIL because `./scripts/bootstrap.sh` does not exist yet.

- [ ] **Step 2: Create `scripts/bootstrap.sh` with explicit checks and reproducible installs**

```bash
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
  just test
  just gateway
EOF
```

Implementation notes:
- create the `scripts/` directory if it does not already exist
- mark the script executable with `chmod +x scripts/bootstrap.sh`
- do not check for `mise` inside the script; the README owns the `mise install` prerequisite flow
- keep runtime config and user bootstrap entirely out of scope

- [ ] **Step 3: Run syntax and behavior verification for the script**

Run:
- `bash -n scripts/bootstrap.sh`
- `just bootstrap`

Expected:
- `bash -n` exits 0
- `just bootstrap` prints tool versions, runs `cargo fetch`, runs `npm ci` in `frontend/`, then prints the next-step guidance

- [ ] **Step 4: Re-run the root command listing to ensure the bootstrap path remains stable**

Run: `just --list`
Expected: PASS with the same five commands still exposed.

- [ ] **Step 5: Commit the bootstrap implementation**

```bash
git add scripts/bootstrap.sh justfile
git commit -m "feat: add dev bootstrap script"
```

## Task 3: Document the Preferred Local Setup Flow

**Files:**
- Modify: `<repo-root>/README.md`

- [ ] **Step 1: Run the red check for missing setup documentation**

Run: `rg -n "mise install|just bootstrap|just test|just gateway" README.md`
Expected: FAIL with no matches because the README does not document the new flow yet.

- [ ] **Step 2: Update the README development section to document the new workflow**

````md
## Development Setup

Install the pinned local toolchain:

```bash
mise install
```

Prepare dependencies:

```bash
just bootstrap
```

Common development commands:

```bash
just test
just gateway
```
````

Implementation notes:
- place this near the existing development commands rather than creating a disconnected appendix
- mention that `build.rs` still provides a fallback frontend install/build path during direct Cargo builds
- do not remove the existing raw commands if they still provide useful low-level detail

- [ ] **Step 3: Run the green check for updated docs**

Run: `rg -n "mise install|just bootstrap|just test|just gateway" README.md`
Expected: PASS with the new setup flow visible in the README.

- [ ] **Step 4: Run the final smoke verification for the documented flow**

Run:
- `just test`
- `cargo run --release -- --help`

Expected:
- `just test` completes backend tests followed by frontend tests
- the CLI help command exits 0, confirming the repo remains runnable after the tooling changes

- [ ] **Step 5: Commit the documentation update**

```bash
git add README.md
git commit -m "docs: add development bootstrap flow"
```
