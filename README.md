# Othello

A dependency-free Rust Othello engine with fixed-depth search, a transposition
table, and a root-only exact endgame solver.

## Build and run

```sh
cargo build --release
cargo run --release -- play
cargo run --release -- perft 8
cargo run --release -- bench --depth 10
cargo run --release -- bench --board 785c68e1f14f0d11:0223171e0e3070a0
```

`play` is human (Black) against the built-in greedy agent. `bench` accepts
`--depth`, `--time-ms`, `--max-nodes`, `--board me:opp`, and
`--endgame-threshold`.

## Strength ladder

Final untouched opening set `bench/openings_final.txt`
(sha256 `9a4082c0…41528ef`), seed 100, 95% pair-bootstrap intervals.

| Match | Pairs | Mean | Interval |
| --- | ---: | ---: | --- |
| tuned vs random (depth 6) | 50 | 1.0000 | uninformative (100 wins) |
| tuned vs greedy (depth 4) | 500 | 0.9840 | [0.9760, 0.9910] |
| tuned vs phase2 (equal depth 4) | 200 | 0.9187 | [0.8925, 0.9425] |
| tuned vs phase2 (equal 5 ms/move) | 400 | 0.9725 | [0.9613, 0.9831] |

Raw logs are in `docs/tuning/100_*.log`.

## Tests and checks

```sh
./scripts/check.sh
cargo test --release -- --ignored
```
