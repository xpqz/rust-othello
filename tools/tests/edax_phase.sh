#!/bin/sh
# The checkpoint analysis survives SIGKILL, takes its active Edax solve down
# with it, and resumes to the same records and summary.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
# Edax lives in the main checkout's ignored scratch/ folder, shared by worktrees.
MAIN=$(CDPATH= cd -- "$(git -C "$ROOT" rev-parse --git-common-dir)/.." && pwd)
EDAX_BIN=${EDAX_BIN:-$MAIN/scratch/edax-reversi/bin/mEdax-native}
test -x "$EDAX_BIN"
TMP=$(mktemp -d "${TMPDIR:-/tmp}/othello-edax-phase.XXXXXX")
# A private copy, so only this test's solver processes match.
mkdir "$TMP/bin"
cp "$EDAX_BIN" "$TMP/bin/mEdax-phase-test"
ln -s "$(dirname "$EDAX_BIN")/data" "$TMP/bin/data"
EDAX="$TMP/bin/mEdax-phase-test"
trap 'pkill -9 -f "$EDAX" 2>/dev/null || true; rm -rf "$TMP"' EXIT
cd "$ROOT"
cargo build --quiet --release --example edax_ladder --example edax_phase \
    --manifest-path tools/Cargo.toml
BIN=tools/target/release/examples

records() {
    if [ -f "$1" ]; then awk 'NR > 1' "$1" | wc -l | tr -d ' '; else echo 0; fi
}
phase() {
    "$BIN/edax_phase" --edax "$EDAX" --journal "$TMP/games.journal" --out "$1" --pairs "$2"
}

"$BIN/edax_ladder" --level 4 --depth 4 --pairs 4 --jobs 4 --max-load 1000 \
    --out "$TMP/games.journal" --edax "$EDAX" \
    --openings bench/openings_tune.txt >/dev/null 2>&1

phase "$TMP/ref.journal" 4 | grep -v '^phase ' >"$TMP/ref.out"
test "$(wc -l <"$TMP/ref.out")" -eq 8

"$BIN/edax_phase" --edax "$EDAX" --journal "$TMP/games.journal" \
    --out "$TMP/kill.journal" --pairs 4 >/dev/null 2>&1 &
pid=$!
tries=0
# Kill only when a 24-empties solve (the long one) has just started, after
# two games are on record.
until [ "$(records "$TMP/kill.journal")" -ge 2 ] && pgrep -f "$EDAX -solve .*-24\.obf" >/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -gt 6000 ] || ! ps -p "$pid" >/dev/null 2>&1; then
        echo "analysis ended or stalled before the kill" >&2
        exit 1
    fi
    sleep 0.01
done
kill -9 "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
if ps -p "$pid" >/dev/null 2>&1; then
    echo "analysis $pid survived SIGKILL" >&2
    exit 1
fi
tries=0
while pgrep -f "$EDAX -solve" >/dev/null; do
    tries=$((tries + 1))
    if [ "$tries" -gt 6 ]; then
        echo "killed analysis left an Edax solve running" >&2
        exit 1
    fi
    sleep 0.05
done
killed=$(records "$TMP/kill.journal")
test "$killed" -ge 2 && test "$killed" -lt 8 || {
    echo "kill left $killed of 8 records" >&2
    exit 1
}
phase "$TMP/kill.journal" 4 | grep -v '^phase ' >"$TMP/kill.out"
awk 'NR > 1' "$TMP/ref.journal" | sort >"$TMP/ref.records"
awk 'NR > 1' "$TMP/kill.journal" | sort >"$TMP/kill.records"
test "$(wc -l <"$TMP/kill.records")" -eq 8
cmp "$TMP/ref.records" "$TMP/kill.records" || {
    echo "resumed records differ" >&2
    exit 1
}
cmp "$TMP/ref.out" "$TMP/kill.out" || {
    echo "resumed summary differs" >&2
    exit 1
}
# Lifetime with a solver that never writes: a stand-in that records its own
# PID and sleeps. Only that PID is ever inspected or signalled.
mkdir -p "$TMP/quiet/data"
printf '#!/bin/sh\necho $$ >"$0.pid"\nexec sleep 600\n' >"$TMP/quiet/solver"
chmod +x "$TMP/quiet/solver"
: >"$TMP/quiet/data/eval.dat"
"$BIN/edax_phase" --edax "$TMP/quiet/solver" --journal "$TMP/games.journal" \
    --out "$TMP/quiet.journal" --pairs 4 >/dev/null 2>&1 &
pid=$!
tries=0
until [ -s "$TMP/quiet/solver.pid" ]; do
    tries=$((tries + 1))
    if [ "$tries" -gt 500 ] || ! ps -p "$pid" >/dev/null 2>&1; then
        echo "stand-in solver never started" >&2
        exit 1
    fi
    sleep 0.01
done
sleeper=$(cat "$TMP/quiet/solver.pid")
kill -9 "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
sleep 0.5
if ps -p "$sleeper" >/dev/null 2>&1; then
    kill -9 "$sleeper" 2>/dev/null || true
    echo "killed analysis left a silent solver running" >&2
    exit 1
fi

if phase "$TMP/kill.journal" 5 >/dev/null 2>&1; then
    echo "accepted a missing pair" >&2
    exit 1
fi
if pgrep -f "$EDAX" >/dev/null; then
    echo "an Edax process outlived the test" >&2
    exit 1
fi
echo "phase check passed: resumed from $killed of 8 records, no solver left"
cat "$TMP/ref.out"
