#!/bin/sh
# Worker protocol, a short two-worker match, SIGKILL and resume, and refusal
# of a journal made with a different configuration.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/othello-version-match.XXXXXX")
cd "$ROOT"
cargo build --quiet --release --example engine_worker --example version_match \
    --manifest-path tools/Cargo.toml
# Private copies, so only this test's workers match.
cp tools/target/release/examples/engine_worker "$TMP/worker-test-a"
cp tools/target/release/examples/engine_worker "$TMP/worker-test-b"
trap 'pkill -9 -f "$TMP/worker-test-" 2>/dev/null || true; rm -rf "$TMP"' EXIT
DRIVER=tools/target/release/examples/version_match

records() {
    if [ -f "$1" ]; then awk 'NR > 1' "$1" | wc -l | tr -d ' '; else echo 0; fi
}
match() {
    "$DRIVER" --a "$TMP/worker-test-a" --b "$TMP/worker-test-b" --time "$1" \
        --pairs "$2" --jobs 2 --max-load 1000 --out "$3"
}

# The worker stops the moment stdin closes, so hold the pipe open for the answer.
reply=$({ echo "go 0000000810000000 0000001008000000"; sleep 1; } | "$TMP/worker-test-a" --time 20)
case "$reply" in
19 | 26 | 37 | 44) ;;
*)
    echo "worker answered $reply from the start position" >&2
    exit 1
    ;;
esac

match 20 2 "$TMP/smoke.journal" >"$TMP/smoke.out" 2>"$TMP/smoke.err"
test "$(records "$TMP/smoke.journal")" -eq 4
grep -q '^a_vs_b pairs 2 games 4 ' "$TMP/smoke.out"

# Started directly, not through the function, so that $! is the driver.
"$DRIVER" --a "$TMP/worker-test-a" --b "$TMP/worker-test-b" --time 20 \
    --pairs 6 --jobs 2 --max-load 1000 --out "$TMP/kill.journal" >/dev/null 2>&1 &
pid=$!
tries=0
until [ "$(records "$TMP/kill.journal")" -ge 2 ] && pgrep -f "$TMP/worker-test-[ab] --time" >/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -gt 6000 ] || ! ps -p "$pid" >/dev/null 2>&1; then
        echo "match ended or stalled before the kill" >&2
        exit 1
    fi
    sleep 0.01
done
kill -9 "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
if ps -p "$pid" >/dev/null 2>&1; then
    echo "driver $pid survived SIGKILL" >&2
    exit 1
fi
tries=0
while pgrep -f "$TMP/worker-test-[ab] --time" >/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -gt 10 ]; then
        echo "killed driver left a worker running" >&2
        exit 1
    fi
    sleep 0.05
done
killed=$(records "$TMP/kill.journal")
test "$killed" -ge 2 && test "$killed" -lt 12 || {
    echo "kill left $killed of 12 records" >&2
    exit 1
}
awk 'NR > 1' "$TMP/kill.journal" | sort >"$TMP/before.records"
match 20 6 "$TMP/kill.journal" >"$TMP/kill.out" 2>"$TMP/kill.err"
test "$(records "$TMP/kill.journal")" -eq 12
grep -q '^a_vs_b pairs 6 games 12 ' "$TMP/kill.out"
awk 'NR > 1' "$TMP/kill.journal" | sort >"$TMP/after.records"
# Complete pairs on record before the kill are kept as they were.
awk -F: '{count[$1]++; line[$1] = line[$1] $0 "\n"} END {for (p in count) if (count[p] == 2) printf "%s", line[p]}' \
    "$TMP/before.records" | sort >"$TMP/kept.records"
test -s "$TMP/kept.records"
test "$(comm -23 "$TMP/kept.records" "$TMP/after.records" | wc -l | tr -d ' ')" -eq 0 || {
    echo "a completed pair changed across the resume" >&2
    exit 1
}

if match 21 6 "$TMP/kill.journal" >/dev/null 2>"$TMP/mismatch.err"; then
    echo "resumed a journal made with another time" >&2
    exit 1
fi
grep -q 'journal header mismatch' "$TMP/mismatch.err"

# The load limit and the seed are part of a journal's identity, to full precision.
custom() {
    "$DRIVER" --a "$TMP/worker-test-a" --b "$TMP/worker-test-b" --time 20 --pairs 1 \
        --jobs 1 --max-load "$1" --seed "$2" --out "$TMP/identity.journal"
}
custom 1000.01 7 >/dev/null 2>&1
for other in "1000.04 7" "1000.01 8"; do
    # shellcheck disable=SC2086
    if custom $other >/dev/null 2>"$TMP/mismatch.err"; then
        echo "resumed a journal made with max-load and seed $other" >&2
        exit 1
    fi
    grep -q 'journal header mismatch' "$TMP/mismatch.err"
done
custom 1000.01 7 >/dev/null 2>&1

# Torn tail: cut into the last record of the finished two-pair journal. The
# other pair survives untouched and the torn pair is replayed once.
torn=$(tail -n 1 "$TMP/smoke.journal" | cut -f1 | cut -d: -f1)
grep -v "^$torn:" "$TMP/smoke.journal" | awk 'NR > 1' | sort >"$TMP/intact.records"
test "$(wc -l <"$TMP/intact.records" | tr -d ' ')" -eq 2
size=$(wc -c <"$TMP/smoke.journal" | tr -d ' ')
head -c $((size - 25)) "$TMP/smoke.journal" >"$TMP/torn.journal"
mv "$TMP/torn.journal" "$TMP/smoke.journal"
match 20 2 "$TMP/smoke.journal" >"$TMP/torn.out" 2>"$TMP/torn.err"
test "$(records "$TMP/smoke.journal")" -eq 4
test "$(awk 'NR > 1' "$TMP/smoke.journal" | cut -f1 | sort -u | wc -l | tr -d ' ')" -eq 4
test "$(awk 'NR > 1' "$TMP/smoke.journal" | sort | comm -12 - "$TMP/intact.records" | wc -l | tr -d ' ')" -eq 2 || {
    echo "the intact pair changed after a torn tail" >&2
    exit 1
}
grep -q '^a_vs_b pairs 2 games 4 ' "$TMP/torn.out"

# Progress is reported on a timer while workers are busy, before any pair ends,
# and carries an estimate once one has.
"$DRIVER" --a "$TMP/worker-test-a" --b "$TMP/worker-test-b" --time 100 --pairs 2 \
    --jobs 1 --max-load 1000 --progress 1 --out "$TMP/progress.journal" \
    >/dev/null 2>"$TMP/progress.err"
grep -q '^progress 0/2 pairs valid, 0/2 played this pass, elapsed [0-9]*s eta unknown$' "$TMP/progress.err"
grep -q '^progress 1/2 pairs valid, 1/2 played this pass, elapsed [0-9]*s eta [0-9]*s$' "$TMP/progress.err"
if pgrep -f "$TMP/worker-test-[ab] --time" >/dev/null; then
    echo "a worker outlived the test" >&2
    exit 1
fi
echo "version match check passed: resumed from $killed of 12 records, no worker left"
grep '^a_vs_b' "$TMP/kill.out"
