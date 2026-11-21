#!/bin/sh
# Book gate (#156), registered one-look run: the play policy with the book
# (`pc:80:c51000v3+book`) against the same without it, one pair per frozen
# root in `openings_book.txt`. The match resumes from its journal, so the
# same command continues after an interruption. Before every launch the
# registered worktree commit, a clean worktree, the opening set and the
# executable's SHA-256 are checked; any mismatch stops the run before a game.
# Run from the main checkout; `--check` verifies the pins and stops.
set -eu
unset OTHELLO_TOOLS_ELAPSED_MS OTHELLO_TOOLS_LOAD

COMMIT=6e532ef2e48b87efa14bb30fc17d7572795bdc60
TOOLS=0c28068213852261cf9db2dcb68c9593bb760211542864e5b24c3169d44003a5
OPENINGS=a819bac0263fec6fa49103f105759c868a58dcaf8391004cdb69bb83f073f53f

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
TREE=$ROOT/../othello-book-gate
LOG=docs/tuning/book_gate.log

fail() {
    echo "book_gate: $*" >&2
    exit 1
}

[ "$(git -C "$TREE" rev-parse HEAD)" = "$COMMIT" ] || fail "$TREE is not at $COMMIT"
# Tracked files only: the run writes its untracked journal and log here.
[ -z "$(git -C "$TREE" status --porcelain --untracked-files=no)" ] || fail "$TREE has local changes"
[ "$(shasum -a 256 "$TREE/bench/openings_book.txt" | cut -c1-64)" = "$OPENINGS" ] ||
    fail "openings_book.txt changed"
target=$("$TREE/scripts/cargo-target.sh")
[ -n "$target" ] && [ -d "$target/release" ] || fail "no build directory (is Space2 mounted?)"
bin=$target/release/othello-tools
[ -x "$bin" ] || fail "missing $bin"
[ "$(shasum -a 256 "$bin" | cut -c1-64)" = "$TOOLS" ] || fail "$bin is not the registered build"
if [ "${1:-}" = "--check" ]; then
    echo "book_gate: all pins hold"
    exit 0
fi

set -- match --a pc:80:c51000v3+book --b pc:80:c51000v3 \
    --openings bench/openings_book.txt --pairs 400 --seed 156 \
    --jobs 4 --max-load 6 --out docs/tuning/book_gate.journal
{
    echo "# start $(date -u +%FT%TZ) commit $COMMIT binary $TOOLS load $(sysctl -n vm.loadavg)"
    echo "# command: othello-tools $*"
} >> "$TREE/$LOG"
cd "$TREE"
"$bin" "$@" >> "$LOG" 2>&1
echo "book_gate: complete"
