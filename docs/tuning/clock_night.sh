#!/bin/sh
# Clock gate (#163), registered night run: the C pilot, the 750 ms pilot,
# and the four tuning matches. Every step resumes from its journal, so the
# same command continues after an interruption; completed steps return at
# once. Before every launch, on every run, the registered worktree commit,
# a clean worktree, the opening set and each executable's SHA-256 are
# checked, and any mismatch stops the run before a game is played. Logs
# append, with a start line per launch. Run from the main checkout;
# `--check` verifies every pin and stops before any game.
set -eu
unset OTHELLO_TOOLS_ELAPSED_MS OTHELLO_TOOLS_LOAD

PILOT_COMMIT=52a20e9f9655ed34bfafb18769302edc6309603c
PILOT_TOOLS=01aca49a4168acae4a48ae938df863ff007c9779e49904968f0fa8b1e7fdb238
TUNE_COMMIT=ca3db5ce8d33660a4a47186131bcdb5e13aa8f0f
TUNE_TOOLS=cb0c1101ce39ec8328ecc64af109da63c5ba629d2ab2fa87d516d0a537e494cc
TUNE_TIME=bd7d42bf8894ca9b62a560b3fa191f15546614a03a78841a41bb414ae0782fc6
OPENINGS=cc2bd7bdeb551fce6c78a1ed60f2d86654e5cf531eb0634a319cdff572544acc

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PILOT=$ROOT/../othello-clock-gate
TUNE=$ROOT/../othello-clock-tune

fail() {
    echo "clock_night: $*" >&2
    exit 1
}

sha() {
    shasum -a 256 "$1" | cut -c1-64
}

# The release directory of a pinned worktree, after checking its commit,
# that it is clean, and its opening set.
release() { # tree commit
    [ "$(git -C "$1" rev-parse HEAD)" = "$2" ] || fail "$1 is not at $2"
    # Tracked files only: the run writes its untracked journals and logs here.
    [ -z "$(git -C "$1" status --porcelain --untracked-files=no)" ] || fail "$1 has local changes"
    [ "$(sha "$1/bench/openings_tune.txt")" = "$OPENINGS" ] || fail "$1: openings_tune.txt changed"
    target=$("$1/scripts/cargo-target.sh")
    [ -n "$target" ] && [ -d "$target/release" ] || fail "$1: no build directory (is Space2 mounted?)"
    echo "$target/release"
}

pinned() { # executable sha
    [ -x "$1" ] || fail "missing $1"
    [ "$(sha "$1")" = "$2" ] || fail "$1 is not the registered build"
}

step() { # tree commit tools-sha log args...
    tree=$1 commit=$2 tools=$3 log=$4
    shift 4
    bin=$(release "$tree" "$commit")
    pinned "$bin/othello-tools" "$tools"
    {
        echo "# start $(date -u +%FT%TZ) commit $commit binary $tools load $(sysctl -n vm.loadavg)"
        echo "# command: othello-tools $*"
    } >> "$tree/$log"
    (cd "$tree" && "$bin/othello-tools" "$@") >> "$tree/$log" 2>&1
}

# `--check`: every pin, then stop before any game.
if [ "${1:-}" = "--check" ]; then
    bin=$(release "$PILOT" "$PILOT_COMMIT")
    pinned "$bin/othello-tools" "$PILOT_TOOLS"
    bin=$(release "$TUNE" "$TUNE_COMMIT")
    pinned "$bin/othello-tools" "$TUNE_TOOLS"
    pinned "$bin/examples/clock_time" "$TUNE_TIME"
    echo "clock_night: all pins hold"
    exit 0
fi

step "$PILOT" "$PILOT_COMMIT" "$PILOT_TOOLS" docs/tuning/clock_pilot.log \
    match --a pc:80:3000 --b pc:80:3000 \
    --openings bench/openings_tune.txt --pairs 50 --seed 1630 \
    --jobs 4 --max-load 6 --out docs/tuning/clock_pilot.journal

step "$TUNE" "$TUNE_COMMIT" "$TUNE_TOOLS" docs/tuning/clock_pilot750.log \
    match --a pc:80:750 --b pc:80:750 \
    --openings bench/openings_tune.txt --pairs 50 --seed 1631 \
    --jobs 4 --max-load 6 --out docs/tuning/clock_pilot750.journal

# C_t: the analyzer must succeed on exactly the registered 50-pair pilot
# and give one positive whole number; it is recorded once and must agree
# on every resume.
bin=$(release "$TUNE" "$TUNE_COMMIT")
pinned "$bin/examples/clock_time" "$TUNE_TIME"
analysis=$("$bin/examples/clock_time" "$TUNE/docs/tuning/clock_pilot750.journal" \
    --a pc:80:750 --b pc:80:750 --pairs 50) || fail "clock_time refused the 750 ms pilot"
CT=$(printf '%s\n' "$analysis" | sed -n 's/^pilot C_ms \([0-9][0-9]*\)$/\1/p')
case "$CT" in
'' | 0 | *[!0-9]*) fail "no positive C_ms from the 750 ms pilot" ;;
esac
RECORD=$TUNE/docs/tuning/clock_ct.txt
if [ -e "$RECORD" ]; then
    [ "$(cat "$RECORD")" = "$CT" ] || fail "C_t $CT differs from the recorded $(cat "$RECORD")"
else
    printf '%s\n' "$analysis" >> "$TUNE/docs/tuning/clock_pilot750.log"
    echo "$CT" > "$RECORD"
fi

for k in 0 1 2 3; do
    step "$TUNE" "$TUNE_COMMIT" "$TUNE_TOOLS" "docs/tuning/clock_tune_v$k.log" \
        match --a "pc:80:c${CT}v$k" --b pc:80:750 \
        --openings bench/openings_tune.txt --pairs 200 --seed $((1640 + k)) \
        --jobs 4 --max-load 6 --out "docs/tuning/clock_tune_v$k.journal"
done
echo "clock_night: complete, C_t $CT"
