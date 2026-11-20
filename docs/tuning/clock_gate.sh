#!/bin/sh
# Clock gate (#163), registered one-look run: the tuning pick
# `pc:80:c51000v3` against today's flat `pc:80:3000`, 400 pairs on the
# untouched `openings_time.txt`. The match resumes from its journal, so the
# same command continues after an interruption. Before every launch the
# registered worktree commit, a clean worktree, the opening set and the
# executable's SHA-256 are checked; any mismatch stops the run before a game.
# Run from the main checkout; `--check` verifies the pins and stops.
set -eu
unset OTHELLO_TOOLS_ELAPSED_MS OTHELLO_TOOLS_LOAD

COMMIT=ca3db5ce8d33660a4a47186131bcdb5e13aa8f0f
TOOLS=cb0c1101ce39ec8328ecc64af109da63c5ba629d2ab2fa87d516d0a537e494cc
OPENINGS=26be800ae9abc8caad531236f1bbb4020f488dd21ade8d0e7525a9bfffa3cb61

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TREE=$ROOT/../othello-clock-tune
LOG=docs/tuning/clock_gate.log

fail() {
    echo "clock_gate: $*" >&2
    exit 1
}

[ "$(git -C "$TREE" rev-parse HEAD)" = "$COMMIT" ] || fail "$TREE is not at $COMMIT"
# Tracked files only: the run writes its untracked journal and log here.
[ -z "$(git -C "$TREE" status --porcelain --untracked-files=no)" ] || fail "$TREE has local changes"
[ "$(shasum -a 256 "$TREE/bench/openings_time.txt" | cut -c1-64)" = "$OPENINGS" ] ||
    fail "openings_time.txt changed"
target=$("$TREE/scripts/cargo-target.sh")
[ -n "$target" ] && [ -d "$target/release" ] || fail "no build directory (is Space2 mounted?)"
bin=$target/release/othello-tools
[ -x "$bin" ] || fail "missing $bin"
[ "$(shasum -a 256 "$bin" | cut -c1-64)" = "$TOOLS" ] || fail "$bin is not the registered build"
if [ "${1:-}" = "--check" ]; then
    echo "clock_gate: all pins hold"
    exit 0
fi

set -- match --a pc:80:c51000v3 --b pc:80:3000 \
    --openings bench/openings_time.txt --pairs 400 --seed 163 \
    --jobs 4 --max-load 6 --out docs/tuning/clock_gate.journal
{
    echo "# start $(date -u +%FT%TZ) commit $COMMIT binary $TOOLS load $(sysctl -n vm.loadavg)"
    echo "# command: othello-tools $*"
} >> "$TREE/$LOG"
cd "$TREE"
"$bin" "$@" >> "$LOG" 2>&1
echo "clock_gate: complete"
