test:
    cargo test

test-verbose:
    cargo test -- --nocapture

fmt:
    cargo fmt

lint:
    cargo clippy -- -D warnings

build:
    cargo build --release

run:
    cargo run --bin mail-orchestrator

docs:
    cargo doc --no-deps --open

audit:
    cargo audit

check-all: fmt lint test build
