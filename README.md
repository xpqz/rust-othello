# Othello

A Rust Othello engine with a terminal front end and an optional egui desktop
GUI. The engine uses iterative-deepening alpha-beta search, move ordering, a
transposition table, and a root-only exact endgame solver.

## Build

```sh
cargo build --release
```

The GUI and tools are separate crates.

## Terminal game

```sh
cargo run --release -- play
```

You play Black and the engine plays White at 3 seconds per move. The board
shows the legal moves; enter one as a square such as `d3`. Forced passes are
automatic. End input with Ctrl-D to quit.

Other CLI commands:

```sh
cargo run --release -- perft 8
cargo run --release -- bench --depth 10
cargo run --release -- bench --board 785c68e1f14f0d11:0223171e0e3070a0
```

`bench` accepts `--depth`, `--time-ms`, `--max-nodes`, `--board me:opp`, and
`--endgame-threshold`.

## Desktop GUI

The GUI is a separate egui/eframe crate:

```sh
cargo run --release --manifest-path gui/Cargo.toml
```

Click a highlighted square to move. You can choose your colour, think time,
and solve threshold, take back a move, or start a new game. See
[`gui/README.md`](gui/README.md) for details.

## Tools

Match and tuning tools live in `tools/`:

```sh
cargo run --release --manifest-path tools/Cargo.toml -- match \
  --a ab:4 --b phase2 --openings bench/openings_final.txt --pairs 200 --seed 100
```

## Tests

```sh
./scripts/check.sh
cargo test --release -- --ignored
```

`./scripts/check.sh` runs formatting, clippy, and the debug test suites.
Release-ignored tests include the endgame oracle and timing benchmarks.

## Layout

- `src/` — engine library and CLI
- `gui/` — optional egui desktop front end
- `tools/` — match harness and tuning utilities
- `tests/`, `bench/` — test suites and frozen positions
- `docs/` — plan, design notes, review records, and benchmark logs

## License

Apache License 2.0. See [`LICENSE`](LICENSE).
