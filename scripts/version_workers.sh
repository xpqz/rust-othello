#!/bin/sh
# Builds tools/examples/engine_worker.rs against two engine commits.
# usage: scripts/version_workers.sh <commit-a> <commit-b> <out-dir>
# Leaves <out-dir>/worker-a, worker-b and provenance.txt.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
A=$(git -C "$ROOT" rev-parse "$1^{commit}")
B=$(git -C "$ROOT" rev-parse "$2^{commit}")
OUT=$3
mkdir -p "$OUT"
OUT=$(CDPATH= cd -- "$OUT" && pwd)
WORKER="$ROOT/tools/examples/engine_worker.rs"
{
    echo "driver_commit $(git -C "$ROOT" rev-parse HEAD)"
    echo "worker_source_sha256 $(shasum -a 256 "$WORKER" | cut -d' ' -f1)"
    echo "rustc $(rustc --version)"
} >"$OUT/provenance.txt"

for side in a b; do
    if [ "$side" = a ]; then commit=$A; else commit=$B; fi
    src="$OUT/src-$side"
    rm -rf "$src"
    mkdir -p "$src"
    git -C "$ROOT" archive "$commit" Cargo.toml Cargo.lock src assets tools/Cargo.toml tools/Cargo.lock tools/src |
        tar -x -C "$src"
    mkdir -p "$src/tools/examples"
    cp "$WORKER" "$src/tools/examples/"
    # The tools crate is its own Cargo root, so the engine manifest's release
    # profile does not apply here. The profile is stated on the command line
    # and the flags rustc was actually given are recorded.
    cargo build --release --locked --verbose --manifest-path "$src/tools/Cargo.toml" \
        --example engine_worker \
        --config 'profile.release.opt-level=3' --config 'profile.release.lto=false' \
        --config 'profile.release.codegen-units=16' --config 'profile.release.debug=false' \
        2>"$OUT/build-$side.log"
    cp "$src/tools/target/release/examples/engine_worker" "$OUT/worker-$side"
    {
        echo "$side commit $commit"
        echo "$side worker_sha256 $(shasum -a 256 "$OUT/worker-$side" | cut -d' ' -f1)"
        echo "$side eval_sha256 $(shasum -a 256 "$src/assets/eval.bin" | cut -d' ' -f1)"
        for crate in othello othello_tools engine_worker; do
            flags=$(grep -- "--crate-name $crate " "$OUT/build-$side.log" | tr ' ' '\n' |
                grep -A1 -- '^-C$' | grep -v -- '^-C$' | grep -v -- '^--$' |
                grep -v '^metadata=\|^extra-filename=\|^incremental=\|^linker=\|^strip=' | sort | tr '\n' ' ' | sed 's/ $//')
            echo "$side rustc_flags $crate: $flags"
        done
    } >>"$OUT/provenance.txt"
done
cat "$OUT/provenance.txt"
