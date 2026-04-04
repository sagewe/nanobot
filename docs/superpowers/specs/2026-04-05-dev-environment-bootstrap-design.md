# Dev Environment Bootstrap Design

Date: 2026-04-05
Repo: `sidekick`
Status: Draft approved for spec write-up

## Summary

Add a small, explicit development-environment bootstrap layer for the Rust + frontend repository using `mise.toml`, `justfile`, and `scripts/bootstrap.sh`. The goal is to make a new local checkout reproducible and easy to start without creating runtime config, mutating `~/.sidekick`, or moving more setup side effects into `build.rs`.

## Goals

- Pin the primary local tool versions for development.
- Expose a small set of stable top-level developer commands.
- Provide a single bootstrap path for dependency checks and first-run installs.
- Keep the runtime bootstrap and operator config flow unchanged.
- Update the README so local setup follows one documented path.

## Non-Goals

- Do not create or modify `~/.sidekick`.
- Do not create application users, profiles, or provider credentials.
- Do not replace Cargo build behavior or remove the existing frontend build fallback in `build.rs`.
- Do not introduce containerized development or OS package installation.
- Do not redesign the test or release workflow.

## Current State

- The repository root is the active Rust runtime and embeds a frontend build.
- The README documents development commands directly as raw `cargo` and `npm` invocations.
- The frontend already uses `npm` and has a lockfile.
- `build.rs` will install frontend dependencies when `frontend/node_modules` is absent, then build the frontend during Cargo builds.
- There is no single source of truth for local tool versions and no single documented bootstrap command.

## Desired State

### Tool Version Source

- Add `mise.toml` at the repository root.
- Use it to declare:
  - `node = "22"`
  - `rust = "stable"`
- Keep version declarations in `mise.toml` only rather than duplicating them in additional tool-version files.

### Command Entry Points

- Add a root `justfile`.
- Keep the initial command surface small:
  - `bootstrap`
  - `test`
  - `frontend-test`
  - `build`
  - `gateway`
- Use `just` as the user-facing command palette, not as a place for complex shell logic.

### Bootstrap Script

- Add `scripts/bootstrap.sh`.
- The script must:
  1. fail fast with `set -euo pipefail`
  2. resolve the repository root from the script location
  3. verify required commands are available on `PATH`
  4. print detected tool versions for troubleshooting
  5. run `cargo fetch`
  6. run `npm ci` in `frontend/`
  7. print the next recommended commands
- The script must not:
  - create runtime config
  - install system packages
  - launch long-running processes
  - silently continue after missing prerequisites

## Scope of Code Changes

### Root Tooling

- Create `mise.toml` with the initial Rust and Node tool declarations.
- Create `justfile` with the developer entry points.

### Shell Script

- Create `scripts/bootstrap.sh` with explicit prerequisite checks and dependency installation.
- Make the script safe to re-run. Repeated runs may re-check or reinstall lockfile-based dependencies, but they must not create new runtime state.

### Documentation

- Update the development section in the README to document the preferred setup flow:
  1. `mise install`
  2. `just bootstrap`
  3. `just test`
  4. `just gateway`
- Note that `build.rs` still provides a fallback frontend install/build path for direct Cargo builds, but it is not the primary onboarding path.

## Command Design

### `just bootstrap`

- Runs `scripts/bootstrap.sh`.
- Serves as the main documented first-run command after `mise install`.

### `just test`

- Runs backend tests, then frontend tests.
- Keeps the cross-stack smoke path in one command.

### `just frontend-test`

- Runs only the frontend test suite inside `frontend/`.

### `just build`

- Runs Cargo build from the repository root.

### `just gateway`

- Runs `cargo run --release -- gateway`.

## Error Handling

- Missing required commands must produce targeted messages that name the missing executable.
- The bootstrap script must stop on the first failing dependency command.
- The script should not attempt to auto-install missing OS dependencies because that couples the repository to machine-specific package managers.
- The README should remain the place that documents the expected manual setup flow when prerequisites are absent.

## Testing Strategy

### Bootstrap Verification

- Confirm `mise install` can resolve the declared tools on a machine that has `mise`.
- Confirm `just bootstrap` completes:
  - command checks
  - `cargo fetch`
  - `frontend/npm ci`

### Developer Command Verification

- Confirm `just test` correctly chains backend and frontend test commands.
- Confirm `just gateway` invokes the expected runtime entrypoint.

### Documentation Verification

- Confirm the README development setup section matches the new command flow exactly.

## Risks and Tradeoffs

- Adding both `mise` and `just` introduces two development tools rather than one, but the split keeps version management and command invocation cleanly separated.
- Pinning `node = "22"` is opinionated. If the project later needs a narrower patch or minor pin, `mise.toml` becomes the single place to change it.
- Retaining the `build.rs` fallback means there are still two ways frontend dependencies can be installed, but one is explicit onboarding and the other is a defensive build-time fallback.
- Using `npm ci` in bootstrap assumes the lockfile remains the source of truth, which is the correct tradeoff for reproducible local setup.

## Acceptance Criteria

- The repository contains `mise.toml`, `justfile`, and `scripts/bootstrap.sh`.
- The bootstrap flow does not create or modify `~/.sidekick`.
- The README documents `mise install` followed by `just bootstrap`.
- `just test` and `just gateway` provide stable root-level entry points for common development tasks.
- The existing Cargo build fallback for frontend install/build remains in place.
