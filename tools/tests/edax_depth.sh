#!/bin/sh
# Fixed-depth games against Edax are deterministic and the header says so.
set -eu

EDAX_BIN=${EDAX_BIN:-/var/folders/5f/46s87cp917n5dfqbcbf_6y540007dl/T/opencode/edax-reversi/bin/mEdax-native}
test -x "$EDAX_BIN"
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/othello-edax-depth.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
cd "$ROOT"
cargo build --quiet --release --example edax_ladder --manifest-path tools/Cargo.toml

for run in 1 2; do
    tools/target/release/examples/edax_ladder --level 4 --depth 4 --pairs 2 --jobs 2 \
        --max-load 1000 --out "$TMP/$run.journal" --edax "$EDAX_BIN" \
        --openings bench/openings_tune.txt >"$TMP/$run.out" 2>"$TMP/$run.err"
    grep -v '^header' "$TMP/$run.journal" |
        sed -E 's/ edax_us [0-9]+//; s/ load [0-9.]+//' | sort >"$TMP/$run.games"
done
head -n 1 "$TMP/1.journal" | grep -q ' engine standard:d4 time none ' || {
    echo "header does not record the depth" >&2
    exit 1
}
test "$(wc -l <"$TMP/1.games")" -eq 4
cmp "$TMP/1.games" "$TMP/2.games" || {
    echo "fixed-depth games differ between runs" >&2
    exit 1
}
echo "depth check passed: $(grep '^level 4 ' "$TMP/1.out" | cut -d' ' -f1-14)"
