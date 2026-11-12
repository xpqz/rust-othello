#!/bin/sh
set -eu

EDAX_BIN=${EDAX_BIN:-/var/folders/5f/46s87cp917n5dfqbcbf_6y540007dl/T/opencode/edax-reversi/bin/mEdax-native}
test -x "$EDAX_BIN"
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/othello-edax-resume.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
cd "$ROOT"

run() {
    cargo run --quiet --release --example edax_ladder \
        --manifest-path tools/Cargo.toml -- \
        --level 1 --pairs 1 --time 10 --jobs 1 \
        --out "$TMP/ladder.journal" \
        --edax "$EDAX_BIN" \
        --openings bench/openings_tune.txt
}

if ! run >"$TMP/first.out" 2>"$TMP/first.err"; then
    echo "first run failed" >&2
    cat "$TMP/first.err" >&2
    exit 1
fi
records_first=$(grep -vc '^header' "$TMP/ladder.journal")
sleep 1
if ! run >"$TMP/second.out" 2>"$TMP/second.err"; then
    echo "second run failed" >&2
    cat "$TMP/second.err" >&2
    exit 1
fi

first=$(grep '^level 1 ' "$TMP/first.out")
second=$(grep '^level 1 ' "$TMP/second.out")
test "$first" = "$second" || {
    echo "summary differs" >&2
    diff "$TMP/first.out" "$TMP/second.out" >&2 || true
    exit 1
}
records_second=$(grep -vc '^header' "$TMP/ladder.journal")
test "$records_first" = "$records_second" || {
    echo "journal records replayed: $records_first then $records_second" >&2
    exit 1
}
echo "resume check passed: $first"
