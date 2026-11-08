# Othello

A dependency-free Rust Othello engine: iterative-deepening alpha-beta search,
move ordering, a 64 MiB transposition table, and a root-only WDL/exact endgame
solver.

## Build

```sh
cargo build --release
```

The engine has no dependencies; release builds use fat LTO and one codegen
unit.

## CLI

```sh
cargo run --release -- play
cargo run --release -- perft 8
cargo run --release -- bench --depth 10
cargo run --release -- bench --board 785c68e1f14f0d11:0223171e0e3070a0
cargo run --release -- bench --board 785c68e1f14f0d11:0223171e0e3070a0 \
  --endgame-threshold 12 --time-ms 1000
```

`play` is Black (human) against White using the search engine at 1 s/move:
the tuned evaluator, 64 MiB TT, and root-only endgame solver. It prints the
board, legal moves, automatic forced passes, and a readable final score:

```text
Game over. Black 0, White 51.
```

`bench` accepts `--depth`, `--time-ms`, `--max-nodes`, `--board me:opp`
(hex bitboards), and `--endgame-threshold`. It reports depth, score, nodes,
nodes/s, elapsed time, solved flag, status, and uses the same 64 MiB TT as
real play.

## Tools

The match and tuning tools live in the separate `tools/` crate:

```sh
cargo run --release --manifest-path tools/Cargo.toml -- match \
  --a ab:4 --b phase2 --openings bench/openings_final.txt \
  --pairs 200 --seed 100
cargo run --release --manifest-path tools/Cargo.toml -- \
  match --a ab:10:5 --b phase2:5 --pairs 400 --seed 100
cargo run --release --manifest-path tools/Cargo.toml --example tune -- \
  no-disc 400 43 2000
```

Matches use paired openings with colours swapped and a pair bootstrap. Agent
names: `random`, `greedy`, `phase2[:ms]`, and `ab:<depth>[:ms]`; `--nodes N`
or a time suffix makes the depth cap 64.

## Conventions and score

- Scores are side-to-move centidiscs (`100` = one disc). Terminal values use
  the WTHOR rule: the winner also gets the empty squares, a tie scores 0, and
  exact scores span `±6400`.
- Black starts on d5/e4, White on d4/e5; Black opens c4/d3/e6/f5.
- Square `i` is `a1 + i`, row-major, `a1` = bit 0.
- A pass consumes one ply in perft and costs no search depth.
- Double-no-move is terminal. Search callers handle pass and terminal roots.
- The exact solver runs only at the root at 12 or fewer empties: WDL in
  `(-1, 1)`, then exact widening. TT endgame bounds are a separate domain.
- Default move budget is 1000 ms; the TT is 4,194,304 16-byte entries
  (64 MiB) and is cleared between agent games.

## Accepted evaluator

Positional neutral table plus phase-weighted features, no disc-difference
term (removed by the #99 direct ablation):

- phase index: linear from 60+ empties (opening) to 12 empties (endgame);
- mobility weight 500 -> 800 per mille;
- frontier multiplier 10 -> 4;
- dynamic X/C corner-risk weight 1000 -> 700 per mille;
- table corners 100, X/C neutral -1, risk 49/19 while the adjacent corner is
  empty;
- output debug-asserted and clamped to `[-6400, 6400]`.

## Strength ladder

Final untouched set `bench/openings_final.txt`, seed 100, 95% pair-bootstrap
intervals. The ladder predates #101's TT integration and was not re-run.

| Match | Pairs | Mean | Interval |
| --- | ---: | ---: | --- |
| tuned vs random (depth 6) | 50 | 1.0000 | all wins |
| tuned vs greedy (depth 4) | 500 | 0.9840 | [0.9760, 0.9910] |
| tuned vs phase2 (equal depth 4) | 200 | 0.9187 | [0.8925, 0.9425] |
| tuned vs phase2 (equal 5 ms, cap 64) | 400 | 0.9163 | [0.8969, 0.9344] |

The equal-time rung uses 5 ms/move for practical run time; strength at the
1 s default is not measured. Logs: `docs/tuning/100_*.log`.

## Performance

Apple M2 Pro, macOS 27.0.1, rustc 1.96.1, release fat LTO with one codegen
unit. Post-TT integration:

| Metric | Result |
| --- | ---: |
| perft(8) | 0.08 s |
| perft(10) | 0.80 s |
| start bench depth 9 | 12.35 ms, 2.64 M nodes/s |
| 1 s search from start | depth 13 |
| 12-empty solve | 21/21, median 13.00 ms |
| 14-empty solve | 9/10, median 85.94 ms |

Details and distributions are in `docs/prs/101.md`.

## Size and product boundary

- Engine + playable CLI: **999 / 999** formatted non-comment lines.
- Tools crate: **632 / 650**.
- Tests, `bench/` data, examples, and logs are outside the production line
  budget.
- No `unsafe` (`forbid` in the library and CLI) and no engine dependencies.

## Tests

```sh
./scripts/check.sh
cargo test --release -- --ignored
```

`./scripts/check.sh` runs fmt, clippy, debug tests, tools tests, and both LOC
guards. Release-ignored tests include the 1,000-position endgame oracle and
timing benchmarks.

## Demo

Scripted and PTY-driven interactive games were both completed against the
release CLI; both finished `Game over. Black 0, White 51.`
