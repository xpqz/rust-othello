#!/usr/bin/env bash
set -euo pipefail

cargo fmt --check
cargo fmt --manifest-path tools/Cargo.toml --check
cargo clippy --all-targets -- -D warnings
cargo clippy --manifest-path tools/Cargo.toml --all-targets -- -D warnings
cargo test
cargo test --manifest-path tools/Cargo.toml
cargo test --release --manifest-path tools/Cargo.toml --example fit_probcut
cargo test --test budget -- --nocapture
cargo test --manifest-path tools/Cargo.toml --test budget -- --nocapture
