#!/bin/sh
# The checkpoint analysis survives SIGKILL and resumes to the same summary.
set -eu

EDAX_BIN=${EDAX_BIN:-/var/folders/5f/46s87cp917n5dfqbcbf_6y540007dl/T/opencode/edax-reversi/bin/mEdax-native}
test -x "$EDAX_BIN"
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/othello-edax-phase.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
cd "$ROOT"
cargo build --quiet --release --example edax_ladder --example edax_phase \
    --manifest-path tools/Cargo.toml
BIN=tools/target/release/examples

records() {
    if [ -f "$1" ]; then awk 'NR > 1' "$1" | wc -l | tr -d ' '; else echo 0; fi
}

"$BIN/edax_ladder" --level 4 --depth 4 --pairs 4 --jobs 4 --max-load 1000 \
    --out "$TMP/games.journal" --edax "$EDAX_BIN" \
    --openings bench/openings_tune.txt >/dev/null 2>&1

"$BIN/edax_phase" --edax "$EDAX_BIN" --journal "$TMP/games.journal" \
    --out "$TMP/ref.journal" --pairs 4 | grep -v '^phase ' >"$TMP/ref.out"
test "$(wc -l <"$TMP/ref.out")" -eq 8

"$BIN/edax_phase" --edax "$EDAX_BIN" --journal "$TMP/games.journal" \
    --out "$TMP/kill.journal" --pairs 4 >/dev/null 2>&1 &
pid=$!
while [ "$(records "$TMP/kill.journal")" -lt 2 ]; do
    sleep 0.1
done
kill -9 "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
if ps -p "$pid" >/dev/null 2>&1; then
    echo "analysis $pid survived SIGKILL" >&2
    exit 1
fi
killed=$(records "$TMP/kill.journal")
test "$killed" -ge 2 && test "$killed" -lt 8 || {
    echo "kill left $killed of 8 records" >&2
    exit 1
}
"$BIN/edax_phase" --edax "$EDAX_BIN" --journal "$TMP/games.journal" \
    --out "$TMP/kill.journal" --pairs 4 | grep -v '^phase ' >"$TMP/kill.out"
test "$(records "$TMP/kill.journal")" -eq 8
cmp "$TMP/ref.out" "$TMP/kill.out" || {
    echo "resumed summary differs" >&2
    exit 1
}
if "$BIN/edax_phase" --edax "$EDAX_BIN" --journal "$TMP/games.journal" \
    --out "$TMP/kill.journal" --pairs 5 >/dev/null 2>&1; then
    echo "accepted a missing pair" >&2
    exit 1
fi
echo "phase check passed: resumed from $killed of 8 records"
cat "$TMP/ref.out"
