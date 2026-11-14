#!/bin/sh
# solve_times: an interrupted run resumes by re-running the same command,
# keeps every completed record unchanged, neither repeats nor omits a
# position, rejects a mismatched header, and redoes a torn last line.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/othello-solve-times.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
cd "$ROOT"
cargo build --quiet --release --example solve_times --manifest-path tools/Cargo.toml
BIN="$ROOT/tools/target/release/examples/solve_times"
ARGS="--empties 12,14 --budget 2000 --previous 20 --progress 1"

ids() { grep -v '^header' "$1" | cut -f1 | sort; }
summary() { grep -E '^(cold|previous_move_warmed) ' "$1"; }

# Uninterrupted reference run: the set of position ids it records.
"$BIN" --out "$TMP/whole.journal" $ARGS >"$TMP/whole.out" 2>"$TMP/whole.err"
total=$(ids "$TMP/whole.journal" | wc -l | tr -d ' ')
test "$total" -gt 300 || { echo "reference run recorded only $total positions" >&2; exit 1; }
test "$(ids "$TMP/whole.journal" | uniq -d | wc -l | tr -d ' ')" = 0 || { echo "reference run repeats an id" >&2; exit 1; }

# SIGKILL part-way, then the same command again.
"$BIN" --out "$TMP/kill.journal" $ARGS >"$TMP/kill1.out" 2>"$TMP/kill1.err" &
pid=$!
for _ in $(seq 1 200); do
    count=$(grep -vc '^header' "$TMP/kill.journal" 2>/dev/null || true)
    test "${count:-0}" -ge 100 && break
    sleep 0.05
done
kill -9 "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
before=$(grep -vc '^header' "$TMP/kill.journal")
test "$before" -lt "$total" || { echo "run finished before it could be killed" >&2; exit 1; }
# Complete records written before the kill (a torn last line is not one).
complete=$(($(wc -l <"$TMP/kill.journal") - 1))
tail -n +2 "$TMP/kill.journal" | head -n "$complete" >"$TMP/kept"
"$BIN" --out "$TMP/kill.journal" $ARGS >"$TMP/kill2.out" 2>"$TMP/kill2.err"
ids "$TMP/whole.journal" >"$TMP/ids.whole"
ids "$TMP/kill.journal" >"$TMP/ids.resumed"
cmp -s "$TMP/ids.whole" "$TMP/ids.resumed" || { echo "resumed run's ids differ from an uninterrupted run's" >&2; diff "$TMP/ids.whole" "$TMP/ids.resumed" | head >&2; exit 1; }
tail -n +2 "$TMP/kill.journal" | head -n "$complete" | cmp -s - "$TMP/kept" || { echo "a record written before the kill changed" >&2; exit 1; }

# The summary is computed from the journal: re-running a finished journal
# solves nothing new and prints the same summary.
"$BIN" --out "$TMP/kill.journal" $ARGS >"$TMP/kill3.out" 2>"$TMP/kill3.err"
summary "$TMP/kill2.out" >"$TMP/s2"; summary "$TMP/kill3.out" >"$TMP/s3"
cmp -s "$TMP/s2" "$TMP/s3" || { echo "summary of a finished journal changed" >&2; exit 1; }
test "$(grep -vc '^header' "$TMP/kill.journal")" = "$total" || { echo "finished journal grew" >&2; exit 1; }

# A mismatched header is a hard error and leaves the journal alone.
cp "$TMP/kill.journal" "$TMP/copy.journal"
if "$BIN" --out "$TMP/kill.journal" --empties 12,14 --budget 2001 --previous 20 >"$TMP/bad.out" 2>"$TMP/bad.err"; then
    echo "mismatched header accepted" >&2; exit 1
fi
grep -q 'journal header mismatch' "$TMP/bad.err" || { echo "no header mismatch message" >&2; exit 1; }
cmp -s "$TMP/kill.journal" "$TMP/copy.journal" || { echo "rejected run changed the journal" >&2; exit 1; }

# A torn last line is discarded and that position redone.
last=$(tail -n 1 "$TMP/kill.journal" | cut -f1)
sed '$d' "$TMP/kill.journal" >"$TMP/torn.journal"
tail -n 1 "$TMP/kill.journal" | cut -c1-25 | tr -d '\n' >>"$TMP/torn.journal"
"$BIN" --out "$TMP/torn.journal" $ARGS >"$TMP/torn.out" 2>"$TMP/torn.err"
ids "$TMP/torn.journal" >"$TMP/ids.torn"
cmp -s "$TMP/ids.whole" "$TMP/ids.torn" || { echo "torn line was not redone exactly once" >&2; exit 1; }
test "$(grep -c "^$last	" "$TMP/torn.journal")" = 1 || { echo "torn position $last not redone" >&2; exit 1; }

echo "solve_times check passed: killed at $before of $total, resumed to $total"
