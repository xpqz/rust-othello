# othello-gui

An optional desktop front end for the engine: you play one colour, the engine
plays the other. It is a separate crate (egui/eframe) and is not part of the
engine's dependency-free, 999-line product.

```sh
cargo run --release --manifest-path gui/Cargo.toml
```

- Click a highlighted square to move; hovering previews the disc.
- The engine searches on a background thread with the shared 64 MiB TT,
  iterative deepening to the think-time budget, and the exact endgame solver
  at or below the "solve at empties" threshold.
- Forced passes happen automatically; "Take back move" returns to your
  previous turn; start a new game as Black or White at any time.
- The last engine search shows its depth and evaluation, or the exact
  solved result.

Keep the solve threshold within what finishes inside the think time: at or
below the threshold the engine only solves, and a timed-out solve falls back
to the first legal move.

## Learning aids

The side panel has two session-persistent toggles, both off by default.
**Opponent move counts** prints on each of your legal moves the number of
legal moves the opponent would have afterwards; an amber-ringed `0` means
the move forces a pass. **Frontier discs** rings your discs that touch at least
one empty square. Prefer moves that leave you few discs touching empty
squares, and leave the opponent few moves.
