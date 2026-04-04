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
