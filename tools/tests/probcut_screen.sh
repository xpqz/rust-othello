#!/bin/sh
# probcut_screen: malformed, repeated, unknown, command-inappropriate and
# value-missing options are rejected before any search.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
cargo build --quiet --release --example probcut_screen --manifest-path tools/Cargo.toml
BIN="$ROOT/tools/target/release/examples/probcut_screen"
reject() {
    if "$BIN" "$@" >/dev/null 2>&1; then
        echo "accepted: $*" >&2
        exit 1
    fi
}
reject nodes --t 80 --t 200
reject nodes --t 80 --typo 9
reject depth --t 80 --ms 0 --jobs
reject depth --t 80 --jobs 0
reject depth --t 80 --jobs 65
reject depth --t 80 --ms 0
reject nodes --t 80 --ms 100
reject nodes
reject nodes --t 30
reject nodes --t 401
reject nodes --t -5
reject bogus --t 80
reject diag
reject diag --t 80 --jobs
reject diag --t 80 --nodes 5
echo "probcut_screen option test passed"
