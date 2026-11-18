#!/bin/sh
# Print the shared Cargo target directory for this checkout on
# /Volumes/Space2 when that volume is really mounted; print nothing
# otherwise, so each crate keeps its own local target/. Sourced into
# CARGO_TARGET_DIR by ~/.zshenv for shells under othello checkouts.
# Override the mount point with OTHELLO_CARGO_VOLUME.

: "${OTHELLO_CARGO_VOLUME:=/Volumes/Space2}"

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

# A stale /Volumes/<name> directory left by an unclean disconnect lives on
# the system disk; a real mount has a different device id from /.
[ -d "$OTHELLO_CARGO_VOLUME" ] || exit 0
[ "$(stat -f %d /)" != "$(stat -f %d "$OTHELLO_CARGO_VOLUME")" ] || exit 0

printf '%s/othello/%s/target\n' "$OTHELLO_CARGO_VOLUME" "$(basename "$root")"
