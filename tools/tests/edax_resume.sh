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


KILL_EDAX="$TMP/mEdax-native"
cp "$EDAX_BIN" "$KILL_EDAX"
KILL_CWD=$(dirname "$EDAX_BIN")
BIN="$ROOT/tools/target/release/examples/edax_ladder"
cargo build --quiet --release --example edax_ladder --manifest-path tools/Cargo.toml

kill_run() {
    "$BIN" --level 1 --pairs 6 --time 150 --jobs 2 \
        --out "$TMP/kill.journal" \
        --edax "$KILL_EDAX" --cwd "$KILL_CWD" \
        --openings bench/openings_tune.txt
}

"$BIN" --level 1 --pairs 6 --time 150 --jobs 2 \
    --out "$TMP/kill.journal" \
    --edax "$KILL_EDAX" --cwd "$KILL_CWD" \
    --openings bench/openings_tune.txt >"$TMP/kill1.out" 2>"$TMP/kill1.err" &
kill_pid=$!
sleep 10
kill -9 "$kill_pid" 2>/dev/null || true
wait "$kill_pid" 2>/dev/null || true
if ps -p "$kill_pid" >/dev/null 2>&1; then
    echo "driver $kill_pid survived SIGKILL" >&2
    exit 1
fi
for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -f "$KILL_EDAX -gtp" >/dev/null || break
    sleep 0.1
done
if pgrep -f "$KILL_EDAX -gtp" >/dev/null; then
    echo "killed driver left an Edax process" >&2
    exit 1
fi
kill_records=$(grep -vc '^header' "$TMP/kill.journal" || true)
test $((kill_records % 2)) -eq 0 || {
    echo "partial pair in journal after kill: $kill_records records" >&2
    exit 1
}
test "$kill_records" -gt 0 || {
    echo "kill case completed no pair" >&2
    exit 1
}
test "$kill_records" -lt 12 || {
    echo "kill case finished before the kill" >&2
    exit 1
}
kill_run >"$TMP/kill2.out" 2>"$TMP/kill2.err"
kill_records=$(grep -vc '^header' "$TMP/kill.journal" || true)
test "$kill_records" -eq 12 || {
    echo "kill resume left $kill_records records" >&2
    exit 1
}
grep -q '^level 1 pairs 6 ' "$TMP/kill2.out" || {
    echo "kill resume summary missing" >&2
    cat "$TMP/kill2.out" >&2
    exit 1
}
echo "kill/resume check passed: $kill_records records"
