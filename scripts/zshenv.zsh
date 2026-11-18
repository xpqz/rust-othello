# Cargo routing for othello checkouts. Install by appending this file
# verbatim to ~/.zshenv; it runs in every zsh, so it can drop an inherited
# routed target once the shell is outside a routed checkout.
#
# OTHELLO_CARGO_ROUTED holds the target this block exported. A
# CARGO_TARGET_DIR equal to it (or unset) is managed and recomputed from
# $PWD and the mount; any other value was chosen by the caller and is kept.
if [[ -z "${CARGO_TARGET_DIR-}" || "${CARGO_TARGET_DIR-}" == "${OTHELLO_CARGO_ROUTED-}" ]]; then
  unset CARGO_TARGET_DIR OTHELLO_CARGO_ROUTED
  if [[ "$PWD" =~ '^(/Users/stefan/work/crust/othello[^/]*)' && -x "$match[1]/scripts/cargo-target.sh" ]]; then
    OTHELLO_CARGO_ROUTED="$("$match[1]/scripts/cargo-target.sh")"
    if [[ -n "$OTHELLO_CARGO_ROUTED" ]]; then
      export CARGO_TARGET_DIR="$OTHELLO_CARGO_ROUTED" OTHELLO_CARGO_ROUTED
    else
      unset OTHELLO_CARGO_ROUTED
    fi
  fi
else
  unset OTHELLO_CARGO_ROUTED
fi
