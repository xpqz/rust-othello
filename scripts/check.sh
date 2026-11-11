#!/usr/bin/env bash
set -euo pipefail

cargo fmt --check
cargo fmt --manifest-path tools/Cargo.toml --check
cargo fmt --manifest-path trainer/Cargo.toml --check
cargo fmt --manifest-path gui/Cargo.toml --check
cargo clippy --all-targets -- -D warnings
cargo clippy --manifest-path tools/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path trainer/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path gui/Cargo.toml --all-targets -- -D warnings
cargo build --manifest-path trainer/Cargo.toml
cargo test
cargo test --manifest-path tools/Cargo.toml
cargo test --manifest-path trainer/Cargo.toml
cargo test --manifest-path gui/Cargo.toml
cargo test --test budget -- --nocapture
cargo test --manifest-path tools/Cargo.toml --test budget -- --nocapture
cargo test --manifest-path trainer/Cargo.toml --test budget -- --nocapture
