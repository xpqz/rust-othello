#!/bin/sh
# probcut_fit collect: a run killed part-way resumes by re-running the same
# command and ends with the same records (timings aside) as an uninterrupted
# run, with no unit repeated or omitted; a larger --nodes extends a finished
# run; a changed header is rejected; a torn last line is redone.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TMP=$(mktemp -d "${TMPDIR:-/tmp}/othello-probcut-fit.XXXXXX")
trap 'rm -rf "$TMP"' EXIT
cd "$ROOT"
cargo build --quiet --release --example probcut_fit --manifest-path tools/Cargo.toml
BIN="$ROOT/tools/target/release/examples/probcut_fit"
ARGS="--depth 6 --deep-depth 7 --deep-every 4 --jobs 3 --seed 7"

# Records without the timing field, sorted by unit id.
records() { grep -v '^header' "$1" | sed 's/ ms [0-9]* / /' | sort -n; }

"$BIN" collect --out "$TMP/whole.journal" --nodes 400 $ARGS 2>/dev/null >/dev/null
test "$(records "$TMP/whole.journal" | wc -l | tr -d ' ')" = 400 || { echo "reference run incomplete" >&2; exit 1; }

# SIGKILL part-way, then the same command again.
"$BIN" collect --out "$TMP/kill.journal" --nodes 400 $ARGS 2>/dev/null >/dev/null &
pid=$!
for _ in $(seq 1 400); do
    count=$(grep -vc '^header' "$TMP/kill.journal" 2>/dev/null || true)
    test "${count:-0}" -ge 60 && break
    sleep 0.02
done
kill -9 "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
before=$(grep -vc '^header' "$TMP/kill.journal")
test "$before" -lt 400 || { echo "run finished before it could be killed" >&2; exit 1; }
# Tear the last line to simulate a kill mid-write.
printf '399\tgame 1 split fit emp' >>"$TMP/kill.journal"
"$BIN" collect --out "$TMP/kill.journal" --nodes 400 $ARGS 2>/dev/null >/dev/null
records "$TMP/whole.journal" >"$TMP/a"
records "$TMP/kill.journal" >"$TMP/b"
cmp -s "$TMP/a" "$TMP/b" || { echo "resumed records differ from the uninterrupted run" >&2; diff "$TMP/a" "$TMP/b" | head >&2; exit 1; }
test "$(cut -f1 "$TMP/b" | uniq -d | wc -l | tr -d ' ')" = 0 || { echo "a unit is repeated" >&2; exit 1; }

# A larger --nodes extends the finished run with the same first records.
"$BIN" collect --out "$TMP/kill.journal" --nodes 450 $ARGS 2>/dev/null >/dev/null
test "$(records "$TMP/kill.journal" | wc -l | tr -d ' ')" = 450 || { echo "extension incomplete" >&2; exit 1; }
records "$TMP/kill.journal" | head -n 400 | cmp -s - "$TMP/a" || { echo "extension changed earlier records" >&2; exit 1; }

# A changed setting is a different header: rejected, journal untouched.
cp "$TMP/kill.journal" "$TMP/saved"
if "$BIN" collect --out "$TMP/kill.journal" --nodes 450 --depth 5 --deep-depth 7 --deep-every 4 --jobs 3 --seed 7 2>/dev/null >/dev/null; then
    echo "mismatched header accepted" >&2; exit 1
fi
cmp -s "$TMP/kill.journal" "$TMP/saved" || { echo "rejected run changed the journal" >&2; exit 1; }
echo "probcut_fit resume test passed ($before records before the kill)"
