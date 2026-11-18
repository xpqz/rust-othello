#!/bin/sh
# Clock gate (#163), registered night run: the C pilot, the 750 ms pilot,
# and the four tuning matches. Every step resumes from its journal, so the
# same command continues after an interruption; completed steps return at
# once. Run from the main checkout. Logs append, with a start line per run.
set -eu
unset OTHELLO_TOOLS_ELAPSED_MS OTHELLO_TOOLS_LOAD
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
PILOT=$ROOT/../othello-clock-gate
TUNE=$ROOT/../othello-clock-tune
step() { # worktree log args...
    tree=$1 log=$2
    shift 2
    bin=$("$tree/scripts/cargo-target.sh")/release
    cd "$tree"
    {
        echo "# start $(date -u +%FT%TZ) commit $(git rev-parse HEAD) binary $(shasum -a 256 "$bin/othello-tools" | cut -c1-64) load $(sysctl -n vm.loadavg)"
        echo "# command: othello-tools $*"
    } >> "$log"
    "$bin/othello-tools" "$@" >> "$log" 2>&1
}
c_of() { # tree journal agent
    bin=$("$1/scripts/cargo-target.sh")/release
    "$bin/examples/clock_time" "$1/$2" --a "$3" --b "$3" --pairs 50 | awk '/^pilot C_ms/ {print $3}'
}
step "$PILOT" docs/tuning/clock_pilot.log match --a pc:80:3000 --b pc:80:3000 \
    --openings bench/openings_tune.txt --pairs 50 --seed 1630 \
    --jobs 4 --max-load 6 --out docs/tuning/clock_pilot.journal
step "$TUNE" docs/tuning/clock_pilot750.log match --a pc:80:750 --b pc:80:750 \
    --openings bench/openings_tune.txt --pairs 50 --seed 1631 \
    --jobs 4 --max-load 6 --out docs/tuning/clock_pilot750.journal
CT=$(c_of "$TUNE" docs/tuning/clock_pilot750.journal pc:80:750)
echo "# C_t $CT" >> "$TUNE/docs/tuning/clock_pilot750.log"
for k in 0 1 2 3; do
    step "$TUNE" "docs/tuning/clock_tune_v$k.log" match --a "pc:80:c${CT}v$k" --b pc:80:750 \
        --openings bench/openings_tune.txt --pairs 200 --seed $((1640 + k)) \
        --jobs 4 --max-load 6 --out "docs/tuning/clock_tune_v$k.journal"
done
