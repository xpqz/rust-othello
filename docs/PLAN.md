# Othello Engine: Implementation Plan

This plan turns [DESIGN.md](DESIGN.md) into phased, testable work. It is the
source of truth for scope, score semantics, file layout, test oracles, and the
order in which things get built. DESIGN.md is historical rationale; where it
conflicts with this file, this file wins.

**Revision 4** resolves the third review: corrected start position, root-only
endgame trigger, an engine+CLI budget boundary with experiment tooling
separated, a three-state search status, deterministic fallback, a single
threshold field, D4/tooling ownership, a production PRNG, budget-metric
loopholes, a Zobrist TT, node-budget tuning, and the smaller contract
fixes. Revision 3 resolved the second review.

**Revision 5** (after release 1.0.0) opens [Phase 2 — Strengthening](#phase-2--strengthening-post-10).
Release 1.0.0 (tag `v1.0.0`) froze the engine + CLI at 998 lines under the
999 ceiling. Phase 2 raises the engine ceiling to 1,499, the tools budget
to 1,300, adds a 1,200-line trainer budget, a model-asset budget, and sets a
strength target. Everything else in this plan still applies.

## Constraints and targets

| Constraint | Target | How it is enforced |
| --- | --- | --- |
| Implementation size | **Hard ceiling: 1,499 LOC for the engine + playable CLI, excluding tests** (Phase 2; 999 in release 1.0.0) | `tests/budget.rs` counts formatted non-comment lines in the `othello` package's production targets |
| Size scope | Engine library and playable CLI in `src/`, plus `build.rs`, `include!`, and `#[path]` sources | Experiment tooling lives in `tools/` with a separate 1,300-line budget (650 in release 1.0.0); the trainer is a separate crate with a 1,200-line budget; tests, `bench/` data, examples, and benchmark data are not engine production source |
| Early feasibility | A complete playable vertical slice with actual counts by the end of Phase 2 | Phase 2 gate records formatted LOC and cuts scope before Phase 3 if needed |
| Dependencies | `std` only for the engine | Engine `Cargo.toml` has no dependencies, guarded by a test; the tools crate may path-depend on the engine; the trainer may use external crates and depends on the engine and tools crates |
| Safety | No `unsafe` in library or binary | `#![forbid(unsafe_code)]` in both `src/lib.rs` and `src/main.rs` |
| Test separation | Zero test code in production targets | All tests under `tests/`; production cannot depend on tests; the budget guard rejects `include!`, `#[path]`, generated `build.rs` source, and formatting-config evasion |
| Speed | perft(8) < 0.2 s, perft(10) < 2 s, search >= 1 M nodes/s, 12-empty solve median < 1 s | Frozen corpus, recorded hardware, release-only checks |
| Strength | Pair-bootstrap interval with a degeneracy policy; candidate beats current accepted config at equal node budget; final set untouched | Tools match harness with frozen position sets |
| Human play | Playable CLI that beats casual human opponents | Phase 8 acceptance run and manual demo |

The engine metric counts formatted non-blank lines that are not comment-only.
Blank lines, `//`, `///`, `//!`, and block-comment lines are excluded; doc
comments are documentation, not code. `scripts/check.sh` runs `cargo fmt
--check` before the budget test, and the budget test rejects `rustfmt.toml` or
`.rustfmt.toml` so formatting cannot compress code. The budget test counts the
source as written; a bare `cargo test` is not a substitute for the formatted
check. `tests/`, `tools/`, `bench/`, examples, and benchmark data are not engine
production source. The tools crate has its own 1,300-line budget and the
trainer crate its own 1,200-line budget. Model
assets have their own budget; see [Phase 2 — Strengthening](#phase-2--strengthening-post-10).

Timing and node-reduction targets are measured on the frozen corpus and
recorded hardware. Default play is 3000 ms per move, but tuning uses fixed node
budgets and short depth/time controls; 400 pairs at the full default budget
would take many hours. Node-budget matches are deterministic and reproducible;
wall-clock runs are reserved for final checks. Optional parallel matches remain
outside the engine budget.

The line ceiling is absolute for the engine + CLI. If a phase would breach it,
cut in this order: WDL-before-margin machinery, optional stability, duplicate
reporting, extra evaluator features, fancy ordering. Production D4 is already
out of the engine; it lives in tests/tools. Parallelism has no engine budget
line. Preserve legal play, score correctness, and endgame strength. Rebenchmark
before removing TT or ordering while claiming unchanged capability; disabling a
runtime flag does not save lines.

## Score and search contracts

These contracts are decided before implementation. Every module uses the same
units and the same abort/terminal rules.

### One score unit: centidiscs

- `SCORE_SCALE = 100`. One disc is 100 centidiscs.
- Every search value, evaluation value, alpha, beta, and terminal value is an
  `i32` in centidiscs.
- `board::final_score` returns WTHOR disc difference in `[-64, 64]`.
  `score::terminal_score(wthor_diff: i32) = wthor_diff * SCORE_SCALE`, so the
  exact terminal range is `[-6400, 6400]`. Phase 0 tests this pure conversion;
  Phase 1 wires it to `board::final_score`.
- `INF = 1_000_000` and `-INF` are safely negatable and far outside any real
  score. Alpha-beta starts from `(-INF, INF)`.
- `EVAL_BOUND = 6_400` centidiscs (64 discs). `evaluate` must return within
  `[-EVAL_BOUND, EVAL_BOUND]`; debug builds assert this and release builds clamp
  as a safety net. This is the same range as an exact terminal score. A
  heuristic value may therefore outrank a small proven win, and a clamped
  `+6400` can tie a proven 64-0 win. This is accepted; exact terminal values are
  never approximated. If exact wins must always dominate, clamp heuristics
  strictly below 6400 or add a WDL layer.
- **Objective:** maximise the WTHOR final score. Among exact terminal values its
  sign encodes the outcome, so a win beats a draw and a draw beats a loss; among
  wins, a larger margin is better. Heuristic values are estimates in the same
  unit. WDL searches compare only the sign; exact margins require widening.

The positional table values from the design are centidiscs: a corner worth 100
is one disc. Feature functions return documented units; `evaluate` applies
weights, keeps the sum inside `EVAL_BOUND`, and returns centidiscs.

### Terminal, pass, and the root

Terminal and pass cases are resolved **before** the depth cutoff. A move
that ends the game exactly at the horizon must return its exact result, never a
heuristic score.

```rust
fn negamax(b: Board, depth: u32, mut alpha: i32, beta: i32, ctx: &mut Ctx)
    -> Option<i32>              // None means aborted, never a score
{
    let moves = legal_moves(b.me, b.opp);
    if moves == 0 {
        if legal_moves(b.opp, b.me) == 0 {
            return Some(terminal_score(final_score(b)));
        }
        return negamax(b.pass(), depth, -beta, -alpha, ctx).map(|v| -v);
    }
    if depth == 0 {
        return Some(ctx.eval(b));
    }

    let mut best = -INF;
    for sq in ordered_moves(b, moves, ctx) {
        let v = -negamax(b.play(sq), depth - 1, -beta, -alpha, ctx)?;
        if v > best { best = v; }
        if v > alpha { alpha = v; }
        if alpha >= beta { break; }
    }
    Some(best)                  // true fail-soft: best found, not incoming alpha
}
```

- A pass costs no depth. A pass is only taken when the side to move has no
  legal move and the opponent does; if neither has a move, the position is
  terminal.
- The search is fail-soft. The returned value is the best value found, even
  when it is below the incoming alpha.
- `None` is the sole cancellation signal. It propagates immediately; callers
  must not convert it to a score.

**Root contract.** `search` requires at least one legal root move and returns
`SearchResult { best: u8, ... }`. The CLI, agents, and game loop handle pass and
terminal roots themselves: if `legal_moves == 0`, the caller passes when the
opponent can move, or ends the game when neither can. A forced-pass root never
enters `search`. If a score is needed at a pass and the passed position has a
legal move, call `search` on `b.pass()` and negate the returned score; use only
the score, not the returned move. This
removes the forced-pass root path and its timeout rules from search.

Test pass and terminal handling through the agent adapter, CLI, and tools game
loop. Do not test a pass root through `search`.

### Iterative deepening and cancellation

`SearchResult` exposes enough state to tell whether the answer is usable:

```rust
enum SearchStatus { Complete, Partial, Fallback }

struct SearchResult {
    best: u8,           // requires a legal root move
    score: i32,
    depth: u32,         // last completed heuristic depth; 0 for a solver root
    nodes: u64,
    status: SearchStatus,
    solved: bool,       // exact root result proven
}
```

- `Complete`: reached `max_depth`, or the root was solved exactly.
- `Partial`: deadline or node limit after at least one completed depth, or a
  completed WDL-only root solve; the result is usable.
- `Fallback`: no completed depth; `best` is the deterministic fallback and
  `score` is the root static evaluation.
- `search` requires at least one legal root move. Callers handle pass and
  terminal roots.
- The deadline clock starts at entry to `search`, including fallback selection
  and ordering. TT clearing and startup allocation are outside; the table is
  cleared between tools harness games, not per search.
- Fallback is the first legal move by square index (lowest index), deterministic.
  There is no greedy alternative.
- If the deadline fires before any iteration completes, return the fallback
  with `status = Fallback`, `depth = 0`, `solved = false`.
- At or below the configured root threshold, the root uses the solver directly:
  no heuristic iterations. Run WDL, then exact. If exact completes, status is
  `Complete`, `depth = 0`, `solved = true`. If only WDL completes, status is
  `Partial`, `solved = false`, `score` is the WDL bound, and `best` is the WDL
  root move. If WDL aborts,
  status is `Fallback`, `depth = 0`, `solved = false`, and `score` is the root
  static evaluation.
- A partial iteration is discarded. Only completed depths update the best move.
  Once the root is proven exact, stop deepening instead of repeating solves.
- Deadline checks happen at node entry and before expensive ordering work.
  `SearchLimits` includes `max_depth`, `time_ms`, and `max_nodes`.
  `SearchConfig` includes the evaluator and `endgame_threshold`; there is
  exactly one threshold field.
- Deterministic cancellation tests use an expired deadline (`time_ms = 0`) or a
  small public `max_nodes` limit. Wall-clock smoke tests allow an overrun
  tolerance of `max(50 ms, 10%)`; they do not assert exact scheduling.
- The previous iteration's best move improves **root** ordering only. Interior
  ordering comes from the TT/PV move; do not claim full-tree ordering from a
  root best move.
- `solve_wdl` returns `Option<i32>` like the main search; a bare `i32` would
  contradict the cancellation channel.

### Transposition table contract

```rust
enum Domain { Heuristic, Endgame }
enum Bound  { Exact, Lower, Upper }

struct Entry {
    key: u64,         // full Zobrist key
    value: i32,       // side-to-move relative, centidiscs
    depth: u8,        // remaining heuristic depth; 0 for endgame entries
    flag: Bound,
    domain: Domain,
    mv: u8,           // 64 = no move
}
```

- Const-fn splitmix64 Zobrist tables `[[u64; 64]; 2]`; `zobrist(board)` XORs
  the keys of occupied squares. Recompute from the bitboards first; use
  incremental updates only if measurement demands them.
- Always-replace. Index with `key % len` or a multiply mix; store the full
  64-bit key and compare it on probe.
- `key == 0` is the empty-slot sentinel. Never store a zero key; a real zero
  key is skipped (negligible).
- A full key detects index collisions but not two positions with the same full
  key. Accept that tiny residual risk in normal play; do not add an oracle
  mode.
- **Strict fixed-depth equivalence:** a heuristic entry is reusable for a
  cutoff only when `entry.depth == depth` and the bound is compatible with the
  current window. A deeper heuristic entry may supply a move for ordering but
  not a score or bound. This preserves cold/warm fixed-depth equivalence.
- `solved` is not a per-entry field; `SearchResult.solved` is the claim about
  the exact root result.
- **Endgame entries** have `domain = Endgame`, `depth = 0`, and are produced by
  the depth-free solver. Exact endgame entries are game-theoretic values.
  Endgame `Lower`/`Upper` bounds may be reused in compatible endgame windows;
  heuristic search does not use endgame entries. A draw proven inside `(-1, 1)`
  may be stored as `Endgame Exact 0`; other WDL results are bounds.
- Heuristic search never consumes endgame entries, and the solver never
  consumes heuristic entries.
- No incomplete result is stored for the interrupted node or its unfinished
  ancestors. Completed sibling subtrees may retain entries; there is no
  rollback or buffering requirement.
- Memory cap 64 MB; entries are about 16 bytes, so about 4 M entries.
- A stored move is validated for legality before being used as root output;
  `mv = 64` means no move.
- A table belongs to one evaluator/search configuration, including evaluator
  identity and `endgame_threshold`. Do not share it across configurations.
  Clear it between tools harness games; do not rely on aging, because old entries can
  remain usable and distort timing comparisons.
- Search with and without a table must agree at fixed depth, including a
  deep-then-shallow sequence with the same warm table.

### Feature contract

- Each feature returns an `i32` in documented units; `evaluate` combines them
  into centidiscs and respects `EVAL_BOUND`.
- **Mobility** uses
  `(m_me - m_opp) * MOBILITY_SCALE / (m_me + m_opp + 1)` with
  `MOBILITY_SCALE = 1000`, so unequal move counts never round to zero.
- **Frontier** returns `opp_frontier - own_frontier` with a positive weight,
  where frontier is `popcount(empty_neighbours & side)`.
- **Corner risk** is dynamic. Replace the design table's static X/C values with
  a neutral base value (for example -1) and apply the X/C penalty only while
  the adjacent corner is empty. When the corner is occupied, the penalty
  disappears.
- Phase endpoints and feature weights are specified in `eval.rs` before
  implementation. The disc-diff feature is active in the heuristic region
  (roughly above the 12-empty solver threshold); tune it on positions where
  the heuristic actually runs, not on solver-only endgames.
- Stability is optional and added last, only if the tools harness shows a gain.
- Evaluation is antisymmetric for the classic evaluator. This deliberately
  rules out a side-to-move tempo term or a parity term. The learned pattern
  evaluator is deliberately not antisymmetric: passing changes whose move it
  is, which is a different position with a different value. Negamax does not
  need antisymmetry. The antisymmetry test stays with the classic evaluator;
  it is not applied to the learned one.

## Decisions resolved from the design's open questions

1. **Final score and objective:** WTHOR score in centidiscs. The winner receives
   empty squares; a tied terminal with empty squares scores zero.
2. **Pass in perft:** a pass consumes one ply and is a node. `perft(b, 0) = 1`;
   a terminal position with `depth > 0` also counts as one node. Depths 1-10
   match Aart Bik's table: `4, 12, 56, 244, 1396, 8200, 55092, 390216,
   3,005,288, 24,571,284`. Terminal leaves first appear at depth 11 in that
   reference, so hand-built early-terminal and pass fixtures supplement perft.
3. **Pass in search:** a pass costs no depth. Terminal detection precedes the
   depth cutoff.
4. **Endgame threshold:** root-only. Solve exactly at 12 empties by default.
   Phase 6 may raise the default to 14-18 based on measured completion rate.
   Perfect play is claimed only when the exact solve completes within the
   deadline.
5. **Time control:** 1000 ms per move for selected checks. Tuning uses fixed
   node budgets; wall-clock runs are final checks. Deadline semantics are
   defined above.
6. **Transposition table:** use const-fn Zobrist tables and full 64-bit keys,
   strict equal-depth heuristic reuse, and a separate endgame domain. Added
   after fixed-depth search, cancellation, and ordering are tested.
7. **Symmetry:** D4 is not production code. Tests and the tools crate own the
   transforms for correctness and canonical position-set keys. Runtime
   augmentation would re-enter the engine budget.
8. **Statistics:** paired positions are the sampling unit. Use a pair bootstrap
   with a finite-sample degeneracy policy; do not treat fractional pair scores
   as binomial successes. Final validation uses an untouched position set.
9. **Baselines:** the tools crate freezes the complete Phase 2 evaluation
   function, not just weights. `phase2` and `release-N` are complete evaluator
   configurations. The engine search accepts an evaluator configuration from
   the start. For search-plus-eval regression claims, retain a versioned engine
   artifact; otherwise label the comparison evaluator-only.
10. **Edax:** out of scope for the 999-line engine. An optional external script
    may drive it later; it never enters production source.
11. **Platform:** the engine plus playable CLI is the 999-line product.
    Experiment tooling (match harness, baselines, position-set generation,
    bootstrap, tuning ladder, logs, `match`) lives in `tools/` with a separate
    1,300-line budget.

## File layout

```
Cargo.toml          engine package, edition 2024, no dependencies
src/
  lib.rs            module wiring, crate policy, shared types
  score.rs          score units, pure terminal conversion, INF and EVAL_BOUND
  board.rs          Board, masks, shifts, legal moves, flips, play, pass, score
  perft.rs          perft tree walk, shared by the CLI and tests
  eval.rs           hand-built evaluation and feature functions
  search.rs         negamax, alpha-beta, ordering, iterative deepening, exact mode
  tt.rs             Zobrist transposition table with domains
  agents.rs         agent trait/factory, random, greedy, search agents
  rng.rs            production xorshift64* PRNG and seed-zero rule
  cli.rs            argument parsing, board printer/parser, play/perft/bench
  main.rs           thin entry point calling cli::run
tests/
  common/mod.rs     helpers, naive reference, D4 test transforms, loaders
  score.rs          pure conversion, bounds, infinity
  rng.rs            xorshift64* known-answer vectors
  movegen.rs        differential move generation and start-position fixture
  perft.rs          exact counts, pass and early-terminal fixtures
  symmetry.rs       D4 orders, composition, commutation
  invariants.rs     board invariants during random playouts
  eval.rs           feature values, antisymmetry, mobility/frontier/corner fixes
  search.rs         minimax, terminal/pass, fail-soft, TT domains, solver, abort
  cli.rs            spawned-binary smoke, malformed input, EOF
  budget.rs         engine LOC ceiling and source-scope guard
  budget/rules.rs   shared line-count and source-scope rules
tools/
  Cargo.toml        separate tools crate, path-depends on othello
  src/
    lib.rs          tools library wiring
    main.rs         match/tune/positions commands
    harness.rs      game loop, factories, side-to-move, validation, logs
    baselines.rs    complete frozen evaluator configurations
    stats.rs        pair bootstrap, degeneracy fallback, Elo
    positions.rs    position-set load/validate and canonical D4 keys
    sym.rs          D4 transforms for canonical keys
    provenance.rs   file hash and engine commit lookup
  tests/
    harness.rs      lifecycle, position splits, bootstrap degeneracy, logs
    golden.rs       frozen v1.0.0 node, move and score fingerprint
    budget.rs       tools LOC ceiling
trainer/
  Cargo.toml        separate trainer crate, path-depends on othello and othello-tools
  README.md         archive download and check instructions
  wthor.sha256      pinned WTHOR file hashes
  src/
    lib.rs          trainer library wiring
    main.rs         check, label, fit and inspect commands
    wthor.rs        WTHOR reader, replay and positions
    check.rs        manifest verification and game summary
    label.rs        label rule, resumable labelling job and journal
  tests/
    cli.rs          command smoke
    wthor.rs        reader, replay, summary and manifest tests
    budget.rs       trainer LOC ceiling
bench/
  corpus.txt        frozen midgame/endgame positions
  openings_tune.txt
  openings_validation.txt
  openings_final.txt
scripts/
  check.sh          fmt + clippy + build + engine/tools/trainer tests + budgets
```

### Module budgets (release 1.0.0 allocation; engine ceiling then 999)

| Module | Budget | Notes |
| --- | ---: | --- |
| `lib.rs` | 10 | Re-exports and policy only |
| `score.rs` | 6 | Units, pure conversion, bounds |
| `board.rs` | 110 | Most care here; plain loops before tricks |
| `perft.rs` | 19 | Tree walk used by the CLI and tests |
| `eval.rs` | 82 | Features plus phase-weighted combination |
| `search.rs` | 407 | Root, ordering, deepening, cancellation, WDL, exact widening |
| `tt.rs` | 114 | Zobrist domain-aware table |
| `agents.rs` | 48 | Trait/factory and small adapters |
| `rng.rs` | 20 | Production PRNG |
| `cli.rs` + `main.rs` | 183 | Printer/parser, play/perft/bench |
| **Total** | **999** | At the ceiling; actual engine is 999 |

This table records the 1.0.0 allocation. Phase 2 issues state their own
per-module estimates within the 1,499 ceiling.

The tools crate has a separate budget (650 in release 1.0.0, 1,300 in Phase 2).
The trainer crate has a separate 1,200-line budget. Their code is not part of
the engine claim and does not consume the engine headroom.

These are estimates, not evidence. Phase 2 proves a vertical slice with actual
formatted counts. Every later phase records cumulative counts.

## Test strategy

Tests are written alongside the phase that introduces the code, but always in
`tests/`. The core library is a normal public API so integration tests need no
special hooks.

**Test layers**

1. **Differential move generation.** A naive `[[i8; 8]; 8]` reference in
   `tests/common/mod.rs` implements the rules by nested loops. The reference
   builds the start position from coordinates itself, not from the production
   constant. Fixed-seed random playouts compare the legal move set and the board
   after every legal move. Run 10,000 positions in normal tests and 200,000
   under `--ignored`. A start-position fixture asserts black's opening moves are
   exactly `{c4, d3, e6, f5}`.
2. **Perft.** Exact counts from the start position at depths 1-10, with the
   direct source link in `tests/perft.rs`:
   `4, 12, 56, 244, 1396, 8200, 55092, 390216, 3,005,288, 24,571,284`.
   Depths 9-10 are release-only `#[ignore]` checks. Add hand-built pass and
   early-terminal fixtures because the initial-position table through depth 10
   does not exercise early game termination. The rule "a terminal position with
   `depth > 0` counts as one node" is this project's convention, not part of
   Aart Bik's depth 1-10 table.
3. **Symmetry.** Test the actual D4 structure: identity order 1, quarter turns
   order 4, half turn order 2, reflections order 2. Check composition, inverses,
   and `legal_moves(transform(b)) == transform(legal_moves(b))`, plus
   `transform(play(b, sq)) == play(transform(b), transform(sq))`. Do not assert
   every transform is an involution.
4. **Invariants.** During random playouts: `me & opp == 0`; total disc count
   increases by exactly one per move; every legal move flips at least one disc;
   two consecutive passes are terminal; a move never changes the opponent's
   disc count except by flips.
5. **Score contract.** Phase 0 tests pure conversion, bounds, `INF`, and
   overflow. Phase 1 tests `terminal_score(final_score(board))` on real boards,
   including a tied terminal with empty squares scoring zero.
6. **Search oracles.** A plain minimax in the tests must agree with alpha-beta
   at depth <= 4 on random positions, including horizon terminals. Explicit
   depth-zero terminal and pass-in-tree tests. A fail-soft test checks that a
   fail-low returns the best child value. A double-no-move terminal fixture
   replaces "mate-in-1": a legal placement causes both sides to have no move,
   with a verified score. A forced-pass root returns `None` from the agent and
   the caller passes; `search` itself is never called on a forced-pass root.
7. **TT contract.** Cold/warm fixed-depth equivalence, including a
   deep-then-shallow sequence with the same table. Heuristic entries are reused
   only at equal remaining depth; deeper entries only order moves. Endgame
   entries and bounds are reused in compatible endgame windows and rejected by
   heuristic search. Preloading shallow heuristic entries does not change the
   solver. A draw inside `(-1, 1)` may be exact zero. Incomplete results are
   not stored; completed siblings may remain. Zobrist index collisions are
   tested by forcing two keys into one slot and checking the full key rejects
   the mismatch. Full-key aliases are accepted residual risk. `mv = 64` is a
   no-move entry. Stored root moves are validated.
8. **Endgame oracle.** Do not claim to test all positions with <= 8 empties.
   Use a documented seeded sample of 1,000 positions with 6-10 empties
   compared against a slow reference exhaustive minimax without the production
   TT or ordering (release-only `#[ignore]`), plus hand-checked known 12-empty
   positions. Assert the solver is root-only: interior nodes use heuristic
   search even below the threshold. Report solve completion rate and p90/p99
   latency as well as the median, and measure fallback strength after an
   interrupted root solve.
9. **Tools harness.** Agents are created per game from a factory/reset
   lifecycle. Agent and bootstrap RNG streams are derived separately from
   stable run/pair/game IDs and logged; there is no opening RNG stream. The game
   loop keeps an absolute side-to-move bit and toggles it on every placement and
   pass, including odd-length opening replay. Release matches reject illegal
   squares and reject `None` when legal moves exist; neither silently becomes a
   pass. Exact 50% tests use deterministic agents; random agents get a
   statistical sanity check. Position sets use canonical D4 keys, reject
   overlap, and record duplicate counts and opening-length distributions. The
   group key is the canonical D4 position after ply 4; all openings in one group
   are assigned to the same split.
10. **Statistics.** Pair bootstrap with a finite-sample degeneracy policy:
    fewer than 10 pairs or zero variance uses a conservative bounded-mean
    interval (for example Hoeffding) and withholds a strength claim; zero pairs
    report insufficient data; one pair reports an uninformative interval; all
    wins/losses/draws use the same policy. Elo endpoints are reported from
    finite interval endpoints; infinity may be a displayed point estimate but
    is never treated as certainty.
11. **Budget guard.** `tests/budget.rs` enforces `<=999` formatted non-comment
    lines for the engine + CLI, rejects `#[cfg(test)]` in production files, and
    rejects implementation hidden in `include!`, `#[path]`, `build.rs`
    generation, or test-only files. It also rejects `rustfmt.toml` and
    `.rustfmt.toml`, and asserts that the engine `Cargo.toml` has no dependency
    table. `tools/tests/budget.rs` enforces the tools 1,300-line budget over
    `tools/src`; committed one-off data-generation scripts in `tools/examples`
    are outside that budget. `trainer/tests/budget.rs` enforces the trainer
    1,200-line budget with the same metric and anti-evasion rules.
12. **CLI.** Validate moves and depth bounds before shifting/indexing, handle
    EOF cleanly, reject malformed input and illegal squares, and define seed
    zero for xorshift. The printer/parser round-trip is exact: parse the printed
    board back to an identical `Board`. The CLI has `play`, `perft`, and
    `bench`; match and tuning live in `tools/`.

**Commands**

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy --manifest-path tools/Cargo.toml --all-targets -- -D warnings
cargo test
cargo test --release -- --ignored
cargo test --manifest-path tools/Cargo.toml
./scripts/check.sh
```

Heavy perft, long differential runs, and full match ladders are `#[ignore]` so
the normal test suite stays fast (target: under 10 seconds).

## Phase overview

| Phase | Outcome | Depends on |
| --- | --- | --- |
| 0 | Scaffold, score contract, conventions, guards | - |
| 1 | Verified board and correctness suite | 0 |
| 2 | Correct fixed-depth search, vertical slice, score contract | 1 |
| 3 | Tools harness, frozen evaluator, position-based splits | 2 |
| 4 | Iterative deepening and deterministic cancellation | 2, 3 |
| 5 | Move ordering and domain-aware TT | 4 |
| 6 | Exact endgame solver and completion metrics | 4, 5 |
| 7 | Evaluator tuning and optional stability | 3, 6 |
| 8 | Validation, performance, release | 5, 6, 7 |

Cross-phase dependencies are explicit: do not start a phase until its
dependencies are green. SKIS links are undirected; use the table above when
selecting work.

---

## Phase 0: Scaffold, score contract, conventions, and guards

**Goal:** A buildable library plus binary, the production LOC metric, the score
contract, and automated guards. No fake stubs for later phases.

**Deliverables**

- `Cargo.toml` with library and binary named `othello`, edition 2024, no
  dependencies, release profile `lto = "fat"` and `codegen-units = 1`.
- `src/lib.rs` and `src/main.rs` both with `#![forbid(unsafe_code)]`.
- Compiling modules: `score`, `board`, `perft`, `eval`, `search`, `tt`,
  `agents`, `rng`, `cli`.
- `src/score.rs`: `SCORE_SCALE`, `INF`, `EVAL_BOUND`, and pure
  `terminal_score(wthor_diff: i32)`. Its doc comments own the objective and the
  heuristic-bound note. No `Board` dependency yet.
- `src/rng.rs`: production xorshift64* PRNG with a defined seed-zero rule.
- `tests/score.rs`: pure conversion, exact range, bounds, negatability, and
  overflow. Do not test `terminal_score(Board)` before Phase 1 delivers it.
- `tests/common/mod.rs` with shared agent types, board formatting helpers, and
  a placeholder naive reference. It imports `rng` rather than defining a
  second PRNG.
- `tests/budget.rs`: count formatted non-comment lines in the engine + CLI;
  assert `<= 999`; reject `#[cfg(test)]` in production files; reject
  implementation hidden in `include!`, `#[path]`, `build.rs`, or tests; reject
  `rustfmt.toml` and `.rustfmt.toml`. Document the comment rule.
- `scripts/check.sh` running fmt, clippy, tests, tools tests, and both budget
  tests.
- Record cross-cutting conventions in `src/lib.rs` doc comments; the score
  objective lives in `src/score.rs`. Mark DESIGN.md sections superseded by
  this plan.

**Tests:** `cargo test` passes; `scripts/check.sh` exits 0; budget test reports
the actual count.

**Gate:** Empty engine builds, pure score-contract tests pass, budget guard
works, and DESIGN.md visibly points to this plan.

**Budget:** <= 60 lines of wiring, PRNG, and score constants.

---

## Phase 1: Verified board and correctness suite

**Goal:** A move generator that is correct by independent arguments and fast
enough to stop worrying about it.

**Deliverables**

- `Board { me: u64, opp: u64 }` with `Clone`, `Copy`, `PartialEq`, `Eq`.
  Square `i` is `a1 + i`, row-major, `a1 = bit 0`, `h8 = bit 63`.
- Initial board constant: black to move with discs on d5/e4 and white on d4/e5,
  expressed as `me = (1 << 28) | (1 << 35)` and
  `opp = (1 << 27) | (1 << 36)` under the stated indexing. Document it. The
  tests build the same position independently from coordinates and assert
  black's legal moves are exactly `{c4, d3, e6, f5}`.
- Direction masks and `shift(bits, dir) -> u64` for the eight directions, using
  `NOT_A` and `NOT_H` exactly as in the design table.
- `legal_moves(me, opp) -> u64`: accumulate opponent runs with at most six
  shifts per direction, then mask landing empties.
- `flips(board, sq) -> u64`: plain per-direction loop with per-step edge checks.
  `play` debug-asserts that the square is legal and flips at least one disc;
  document illegal-square preconditions.
- `pass`, `is_terminal`, `disc_count`, `empty_count`, and
  `final_score -> i32` in WTHOR disc units. Wire
  `score::terminal_score(final_score(b))` and test it here, including a tied
  terminal with empty squares scoring zero.
- `src/perft.rs`: `perft(board, depth) -> u64` with the documented pass
  convention, shared by the CLI and tests.
- `tests/common/mod.rs`: naive `[[i8; 8]; 8]` reference and test-only D4
  transforms for canonical keys.
- Test files `tests/movegen.rs`, `tests/perft.rs`, `tests/symmetry.rs`,
  `tests/invariants.rs`.

**Implementation notes**

- Write the naive reference first, then the bitboard version against it. Build
  the reference start from coordinates so a production constant cannot mask a
  swapped-colour bug.
- Keep `bit(sq) = 1u64 << sq` and avoid scattered `usize` casts.
- Perft depths 1-10 use Aart Bik's table; add hand-built pass and early-terminal
  fixtures because the initial table through depth 10 is pass-free. The
  terminal-with-depth convention is ours, not Bik's.
- Add a `bench/corpus.txt` placeholder; Phase 5 fills it.

**Tests and acceptance**

- 10,000 random positions: legal sets and post-move boards match the reference.
- Perft depths 1-8 exact; ignored 9-10 exact.
- Hand-built pass/early-terminal perft fixtures pass.
- D4 orders, composition, and commutation pass.
- Invariants hold across every random playout.
- Release perft(8) under 0.2 s on recorded hardware; record nodes/s.

**Gate:** Differential, perft, symmetry, invariants, and terminal-score wiring
are green. Record cumulative formatted LOC. No search code before this gate.

**Budget:** `board.rs` <= 130, `perft.rs` <= 20.

---

## Phase 2: Correct fixed-depth search, vertical slice, and score contract

**Goal:** A playable engine with a correct fixed-depth alpha-beta search, an
evaluator configuration passed in from the start, and actual LOC evidence.

**Deliverables**

- `src/eval.rs`: a minimal `evaluate(board) -> i32` in centidiscs using the
  positional table plus disc difference, bounded by `EVAL_BOUND`.
- A search configuration type carrying the evaluator function pointer and
  endgame threshold, so two evaluators can coexist:
  `SearchConfig { eval: fn(Board) -> i32, endgame_threshold: u32, ... }`.
- `src/search.rs`:
  - `negamax(board, depth, mut alpha, beta, ctx) -> Option<i32>` with terminal
    and pass handling **before** the depth cutoff, pass costing no depth, and
    true fail-soft. Interior nodes never call the solver.
  - Fixed-depth `search` returning `SearchResult { best: u8, ... }` and
    requiring at least one legal root move.
  - Deterministic first-legal fallback and `status`/`solved` fields.
- `src/agents.rs`: an `Agent` trait or factory returning `Option<u8>`, plus
  `random_agent(seed)`, `greedy_agent()`, and `search_agent(config, limits)`.
  `None` means no legal placement at the root; the caller passes or ends the
  game. A forced-pass root never reaches `search`.
- `src/cli.rs` and `src/main.rs`: board printer and exact parser, play
  (human vs agent, `d3`/`pass`), `perft`, `bench`. Validate input before
  shifting/indexing; handle EOF cleanly.
- `tests/search.rs`, `tests/score.rs`, `tests/cli.rs` additions.

**Implementation notes**

- Track `best` separately from `alpha`; never return incoming alpha on fail-low.
- Terminal detection calls `legal_moves` for both sides.
- `score::terminal_score` is the only disc-to-centidisc conversion.
- The CLI is a thin adapter; testable logic lives in the library.
- Printer/parser round-trip is exact: printed board parsed back equals the
  original `Board`.

**Tests and acceptance**

- Depth-zero terminal, pass-in-tree, and horizon terminal.
- Fail-soft: a position where every child is below alpha returns the best child
  value.
- Plain minimax agrees with alpha-beta at depth <= 4 on 100 positions.
- Double-no-move terminal fixture: a legal placement causes both sides to have
  no move, with a verified score.
- Forced-pass root: the agent returns `None`; the CLI passes; `search` is not
  called. Terminal root ends the game.
- Score bounds and overflow.
- CLI smoke, malformed input, illegal square, EOF, and printer round-trip.
- **Vertical slice:** a complete scripted game from the CLI with actual
  formatted LOC recorded.

**Gate:** Fixed-depth search is correct; pass and terminal roots are handled by
callers; the vertical slice works; actual LOC is recorded and, if over
projection, scope is cut before Phase 3.

**Budget:** `eval.rs` <= 60 at this stage, `search.rs` <= 110, `agents.rs` <=
50, `cli.rs` + `main.rs` <= 170.

---

## Phase 3: Tools harness, frozen evaluator, position-based splits

**Goal:** Reproducible paired games against a genuinely frozen evaluator, with
position-based held-out sets and action validation, in a separate tools crate.

**Deliverables**

- `tools/src/harness.rs`:
  - `play_game` over agents created from factories for each game.
  - Absolute side-to-move bit outside the side-relative `Board`; toggle on
    every placement and pass, including odd-length opening replay. Use it for
    agent selection, colour printing, and terminal attribution.
  - One frozen opening source: the position sets in `bench/`. No seeded
    opening generation and no opening RNG stream.
  - Each opening is played twice with colours swapped; the pair is the sampling
    unit. Pair score is the mean of two game scores (win=1, draw=0.5, loss=0).
  - Separate RNG streams for agents and bootstrap, derived from stable
    run/pair/game IDs and logged.
  - Release-mode action validation: reject illegal squares and reject `None`
    when legal moves exist; neither silently becomes a pass.
  - Pair bootstrap with the degeneracy policy described below.
  - Serial runner only; parallel matches are optional in Phase 8.
- `tools/src/baselines.rs`: a complete frozen `eval_phase2(board)` function,
  including table, scaling, rounding, and clamping, plus a fixed-depth
  configuration. `phase2` later uses this evaluator with current search.
  Released evaluators are added as `release-N`.
- Position sets in `bench/openings_tune.txt`,
  `bench/openings_validation.txt`, and `bench/openings_final.txt`, with a
  declared serialization and canonical D4 key. Reject overlap; record duplicate
  counts and opening-length distributions. The group key is the canonical D4
  position after ply 4; all openings in one group go to one split. Tuning uses
  `tune`; internal checks use `validation`; `final` is untouched until release.
  If `validation` is inspected and the engine changes in response, mark it
  developmental and use a fresh final set.
- `tools/src/main.rs` commands `match`, `tune`, and `positions`; `match` takes
  `--a`, `--b`, `--pairs`, `--seed`, `--openings`, `--depth`, `--nodes`, and
  agent names `random`, `greedy`, `ab:<depth>`, `phase2`.
- `tools/tests/harness.rs`.

### Statistics policy

- Pair score is the sampling unit.
- With at least 10 pairs and non-zero variance, report a percentile pair
  bootstrap interval (for example, 10,000 resamples, seeded).
- With fewer than 10 pairs or zero variance, use a conservative bounded-mean
  interval (for example Hoeffding) and label it uninformative for strength
  claims. Zero pairs report insufficient data. One pair reports an
  uninformative interval. All-wins/all-losses/all-draws use the same fallback.
- Elo point estimates come from the mean pair score; interval endpoints come
  from finite score endpoints. Infinity may be displayed as a point estimate
  but never treated as certainty.
- A 55% equal-time claim needs about 400 pairs; a smaller run that misses 55%
  is inconclusive, not evidence against the engine. Equal-node runs use the
  same pair count when making a strength claim.
- Exact 50% tests use deterministic agents replaying the same opening. Random
  agents get a statistical sanity check, not an exact equality assertion.

**Tests and acceptance**

- Fixed-node logs and results are byte-identical on repeat; wall-clock runs
  are not promised byte-identical.
- Identical deterministic agents score 50% over colour-swapped pairs; random
  agents pass a statistical sanity check.
- Position sets have no canonical-key overlap; duplicate and length stats are
  recorded; group assignment is split-consistent.
- Agent factories create fresh state per game; TT and counters do not leak.
- Release-mode illegal actions abort the game with an error.
- Bootstrap degeneracy cases and Elo endpoints behave as specified.
- Baseline ladder: random vs random, greedy vs random, `phase2` vs greedy.

**Gate:** `tools match --a phase2 --b greedy --pairs 200 --seed 1` is
reproducible and prints a valid interval. The frozen evaluator is a complete
function, position splits are valid, and tools harness tests pass.

**Budget:** tools crate <= 650 lines total; `harness.rs` <= 180,
`baselines.rs` <= 40, `stats.rs` <= 120, `positions.rs` <= 120.

---

## Phase 4: Iterative deepening and deterministic cancellation

**Goal:** Use the time budget safely, with a cancellation contract that is
testable without relying on wall-clock luck.

**Deliverables**

- Iterative deepening; previous-iteration best at the root only.
- `SearchResult` with `best: u8`, `score`, `depth`, `nodes`, `status`, `solved`.
- `SearchStatus { Complete, Partial, Fallback }`; deterministic first-legal
  fallback. `search` requires a legal root move.
- `SearchLimits` with `max_depth`, `time_ms`, and `max_nodes`. The single
  `endgame_threshold` field lives in `SearchConfig`.
- Deadline clock starts at `search` entry, including fallback selection and
  ordering. TT clearing and startup allocation are outside.
- Recursive cancellation via `Option<i32>`; no incomplete result stored for the
  interrupted node or its unfinished ancestors; completed siblings may remain.
- Stop deepening once the root is proven exact.
- CLI `--time-ms`, `--depth`, `--max-nodes`; `bench` reports nodes, nps, depth,
  and elapsed time. Tools agent naming uses `ab:<depth>:<time-ms>` and
  `phase2:<time-ms>`; fixed-node matches use `--nodes`.
- Tests: ID vs fixed depth; deterministic cancellation with `time_ms = 0` and
  with a small `max_nodes`; tolerant wall-clock smoke (50 ms budget, overrun
  tolerance `max(50 ms, 10%)`); pass and terminal roots handled by callers;
  returned moves always legal; `status` alone tells usability.

**Implementation notes**

- Do not put a solver-timeout integration test here; the solver lands in
  Phase 6.
- A root best move alone improves root ordering only.
- Count nodes consistently and use the same counter for `bench` and ordering.

**Gate:** ID and fixed depth agree; deterministic cancellation tests pass;
wall-clock smoke stays within tolerance; status/solved semantics are correct;
equal-node matches run.

**Budget:** `search.rs` <= 130 before the table.

---

## Phase 5: Move ordering and domain-aware transposition table

**Goal:** Reach useful depth by pruning and reuse positions without violating
fixed-depth equivalence or confusing heuristic and endgame searches.

**Deliverables**

- Ordering: root previous best; interior TT/PV move; corners; moves minimising
  opponent replies only at depth >= 2; X/C last. Fixed `[u8; 64]` buffer,
  in-place sort, no hot-path allocation. Precompute mobility-ordering keys once
  per move if comparator recomputation is expensive.
- `src/tt.rs`:
  - Const-fn splitmix64 Zobrist tables `[[u64; 64]; 2]`; `zobrist(board)` XORs
    occupied squares.
  - `Entry { key, value, depth, flag, domain, mv }`; 64 MB cap;
    always-replace; full key stored.
  - `key == 0` empty sentinel; a real zero key is not stored.
  - `mv = 64` means no move.
  - Strict equal-depth heuristic reuse; deeper heuristic entries order only.
    Endgame domain entries and bounds reusable in endgame windows; heuristic
    search never consumes endgame entries.
  - No incomplete stores; completed siblings may remain.
  - Stored move validated for legality before root output.
- Integrate TT into `negamax`.
- `bench/corpus.txt`: frozen midgame and endgame positions, including pass
  positions and difficult endgames, with a documented serialization and
  expected score provenance. Record warm/cold TT conditions.
- Tests: cold/warm fixed-depth equivalence, including deep-then-shallow; domain
  separation; endgame bound reuse; preloaded heuristic entries; abort stores
  local rule; forced Zobrist index collision; node and time-to-depth
  improvements on the corpus.

**Implementation notes**

- Bound-type and domain handling are the risky parts. Test each store case.
- Record nodes, fixed-depth time, completed depth, and time-to-depth. Node
  reduction alone can conceal expensive ordering.
- Recomputing Zobrist per node and polling `Instant` every node are first
  implementations, not established 1 M nodes/s. Profile before adding
  machinery.
- Shrinking the TT saves memory, not source lines.
- Rebenchmark before removing TT or ordering while claiming unchanged
  endgame capability.

**Gate:** Fixed-depth equivalence green including deep-then-shallow; domain and
bound tests green; ordering cuts nodes by at least 40% on the corpus without a
time-to-depth regression; TT cuts nodes by at least 20% without a time
regression. These are initial targets; record the corpus/hardware baseline and
adjust the gate if the corpus shows a different scale.

**Budget:** `tt.rs` <= 110, `search.rs` <= 290.

---

## Phase 6: Exact endgame solver and completion metrics

**Goal:** Perfect play from about 12 empties when the solve completes, with
reusable endgame bounds and honest completion reporting.

**Deliverables**

- `solve(board, alpha, beta, ctx) -> Option<i32>` in `src/search.rs`, reusing
  `play`, ordering, and the TT. No depth parameter; recurse to terminal,
  passing when no moves.
- Endgame domain entries: exact game-theoretic values and bounds; `depth = 0`.
  Heuristic search never consumes them. A draw proven inside `(-1, 1)` may be
  stored as `Endgame Exact 0`; other WDL results are `Lower`/`Upper`.
- `solve_wdl -> Option<i32>` with `(-1, 1)` centidiscs, widened to
  `(-INF, INF)` for exact margins.
- **Root-only trigger:** when the root has `empty_count <= endgame_threshold`,
  run WDL then exact, with no heuristic iterations. Interior nodes never call
  the solver. Default threshold 14; Phase 6 may raise it to 14-18 based on
  measured completion.
- Timeout integration: an aborted root solve returns the fallback with
  `solved = false`; a completed WDL-only solve is `Partial`. Test this here,
  not in Phase 4.
- Stop iterative deepening once the root is proven exact.
- Tests: documented 6-10 empty sample vs slow reference minimax (release-only
  `#[ignore]`); known 12-empty positions; pass/terminal; TT preloaded with
  shallow heuristic entries; WDL bound semantics and exact-zero draw; solver
  timeout/fallback; interior nodes remain heuristic below the threshold.
- Benchmarks: 12 and 14 empties on the corpus; report median, p90, p99, solve
  completion rate, nodes/s, and fallback strength after an interrupted first
  solve, with hardware/build context.

**Gate:** Oracle sample and known positions green; 12-empty median < 1 s on
recorded hardware; completion rate and p90/p99 recorded; full game with solver
finishes under the CLI time limit; aborted solves report `solved = false`.

**Budget:** `search.rs` <= 407 including WDL and exact widening;
`tt.rs` unchanged.

---

## Phase 7: Evaluator tuning and optional stability

**Goal:** A correct evaluator that beats the current accepted configuration at
equal node budget, with gains attributed by ablation rather than a single
baseline win.

**Deliverables**

- Feature functions with the corrected mobility, frontier, and dynamic corner
  risk. Specify phase endpoints and feature weights before implementation.
- Disc-diff was removed by #99's direct ablation (no-disc 0.773 vs base);
  retained features (mobility, frontier, corner risk) have direct ablations.
- Optional conservative stability, added last and kept only if an ablation
  shows a gain.
- `tests/eval.rs`: hand-computed values, unequal mobility counts, frontier
  antisymmetry, corner-risk disappearance, eval antisymmetry, phase endpoints,
  bounded output.
- Tuning protocol:
  - Compare each candidate with the **immediately preceding accepted
    configuration**, under identical search settings and node budget.
  - Use an ablation to attribute gain to a feature.
  - Predeclare the selection protocol and the number of candidates before
    looking at results.
  - Use the frozen openings in `tune`, alternating colours, 4-8 plies, and
    fixed node budgets. Broad scans may use 200 pairs; a 55% equal-time claim
    needs about 400 pairs, and a smaller miss is inconclusive.
  - Keep a change only if the pair-bootstrap interval clears the predeclared
    threshold. A positive Elo interval is a monotonic transform, not independent
    confirmation.
  - Record run/pair/game IDs, position-set hash, evaluator config, depth, node
    budget or time control, score, interval, and Elo.
  - Validate the final chosen configuration on `validation`; do not tune on it.
    If validation is used to change the engine, demote it and use `final`.

**Gate:** Feature tests green; the selected evaluator beats the current accepted
configuration at equal node budget with a valid interval; each retained feature
has an ablation result; the final configuration is validated on an untouched
set.

**Budget:** `eval.rs` <= 86 (reduced by #91/#93; 25 actual before Phase 7).

---

## Phase 8: Validation, performance, and release

**Goal:** Prove the stated targets and hand over a playable, maintainable engine
with honest evidence.

**Deliverables**

- Strength ladder on frozen position sets, with pair bootstrap and declared
  node budgets/time controls:
  - 500 pairs vs greedy: target >= 95%.
  - 200 pairs vs `phase2` at equal depth: target >= 65%.
  - 400 pairs vs `phase2` at equal time: target >= 55% with the interval
    clearing 50%.
  - 200 pairs vs the previous frozen configuration: retain a versioned engine
    artifact for a search-plus-eval claim, or label the comparison
    evaluator-only.
  - 50 pairs vs random: target >= 99%.
- Performance evidence: hardware, OS, Rust version, build settings, frozen
  corpus hash, warm/cold TT conditions, perft(8/10), midgame nps,
  fixed-depth time, completed depth in a fixed budget, 12/14-empty solve
  completion and p90/p99 latency, and fallback strength.
- Optional parallel matches: preserve position-pair units, derive per-pair seeds
  from stable IDs, and control CPU contention; serial remains the reference.
- Quality gate: fmt, clippy, tests, ignored release tests, `scripts/check.sh`.
- LOC audit: `tests/budget.rs` confirms `<=999` formatted non-comment lines
  for the engine + CLI; `tools/tests/budget.rs` confirms the tools budget. No
  test-code leakage, `include!`, `#[path]`, `build.rs` generation, or
  formatting-config escape. If over, cut in the agreed order and re-run
  affected benchmarks.
- Docs: README with build, CLI, score units, conventions, strength evidence,
  hardware context, actual engine/tools line counts, and product boundary.
  Update this plan with any changed decisions.
- Manual demo: scripted and interactive games.

**Acceptance checklist**

- [x] All correctness tests pass, including differential, perft, pass/terminal
      fixtures, and the start-position fixture.
- [x] Score contract, fail-soft, double-no-move terminal, and caller-handled
      pass/terminal roots pass.
- [x] TT strict fixed-depth equivalence, deep-then-shallow, domains, bounds,
      local abort rule, and stored-move legality pass.
- [x] Endgame oracle uses the documented 6-10 empty sample and known 12-empty
      positions, with completion rate and p90/p99.
- [x] Tools harness uses per-game agent factories, absolute side-to-move,
      release action validation, position-based splits, and bootstrap
      degeneracy.
- [x] Engine + CLI source is `<=999` formatted non-comment lines; tools are
      `<=650`; no test code in production files.
- [x] No dependencies, no `unsafe`, fmt and clippy clean.
- [x] perft(8) < 0.2 s, perft(10) < 2 s, search >= 1 M nodes/s, 12-empty median
      < 1 s on recorded hardware.
- [x] Strength thresholds met with recorded intervals, seeds/IDs, depth, and
      node budget/time control; final set untouched.
- [x] CLI plays a complete game, handles EOF/malformed input, and prints a
      readable final score.
- [x] README documents build, play, benchmark, tools match, conventions, and
      actual engine/tools sizes.

**Gate:** Every checkbox is satisfied. This is the release.

Released as 1.0.0 (tag `v1.0.0`): engine + CLI 998 lines, tools 639.

---

## Phase 2 — Strengthening (post-1.0)

Phases 0-8 above are the internal phases of the 1.0 plan. "Phase 2" here is
the project's second stage: making the engine substantially stronger, within
a new budget and with the same discipline.

### Budget

| Item | 1.0.0 | Phase 2 | Enforced by |
| --- | ---: | ---: | --- |
| Engine + playable CLI | 999 | **1,499** | `tests/budget.rs` (same metric) |
| Tools crate | 650 | **1,300** | `tools/tests/budget.rs` |
| Trainer crate | — | **1,200** | `trainer/tests/budget.rs` |
| Model assets | — | **4 MiB** total | Guard added with the first asset |

- **Same metric, same rules:** formatted lines that are neither blank nor
  comment-only. The anti-evasion rules (no formatter config, no `include!`,
  `#[path]` or generated source) all stay.
- **Model assets** are generated data such as evaluation weights or an
  opening book. They live under `assets/`, must be reproducible from the
  trainer crate with documented inputs and seed, and do not count
  as lines. The code that loads or uses them counts as engine lines. The
  include guard currently rejects `include_bytes!`. The issue that adds the
  first asset relaxes it for files under `assets/` only, with a test, and adds
  the 4 MiB size check.
- **`tools/examples` stays uncounted** data-generation code (decision
  045ea9b). Training and fitting pipelines live in the counted `trainer/`
  crate.

### Why 1,499

The ceiling covers the candidate work below with little to spare, so the
priorities still have to be chosen. Estimates are formatted engine lines:

| Candidate | Est. lines | Notes |
| --- | ---: | --- |
| ProbCut (v1: full-width probes, selective TT domain) | 50-70 | Deferred; preserved by tag `probcut-v1` |
| PVS and aspiration windows | 20-30 | Narrower root and child windows |
| Endgame speed (parity ordering, fastest-first, small-empty solvers) | 60-90 | Lets the exact threshold rise above 14 |
| Learned pattern evaluation | 120-180 net | Replaces most of `eval.rs`; weights are an asset |
| Lazy SMP with an atomic TT | 70-100 | `std::thread` only; no `unsafe` |
| Opening book | 40-60 | Book data is an asset |
| Time management | 20-30 | Per-move allocation from game stage |

The total is about 380-560 lines against 500 of headroom. Doing everything at
the top of each range would not fit, so later items are cut or simplified
first.

### Strength target

Phase 2 is complete when all of the following hold:

1. **Gate:** at 3 s per move, the Phase 2 engine beats the frozen 1.0.0
   engine by a pair-bootstrap Elo lower bound of at least **+200**, over at
   least 200 pairs from `bench/openings_final.txt`. The final set is used for
   this check only. The 1.0.0 opponent is built from tag `v1.0.0`
   (a versioned engine artifact, per decision 9), not from the current
   source with features switched off.
2. **Reported, not gated:** the score against single-threaded Edax at equal
   time over the same openings. 1.0.0 scored 1 win, 1 draw and 6 losses in an
   informal 8-game match. The stretch goal is at least 40% against Edax.
3. **Unchanged guarantees:** exact play from the endgame threshold, legal play
   under every limit, `std`-only, no `unsafe`, fmt and clippy clean.

### Progress

ProbCut v1 (full-width probes, selective TT domain) failed validation twice:
first equal nodes +18 Elo [-8.69, 46.31]; the second attempt passed equal
nodes (0.5220, [0.5055, 0.5383]) and 500 ms (0.5240, [0.5075, 0.5403]) but
failed the 3 s check (0.4825, [0.4350, 0.5325]). The switch was never enabled.
The implementation is preserved by tag `probcut-v1` (095f4e4) and has been
removed from the engine/tools. A second version waits for the new evaluator.
Both validation sets have been seen; later work is a declared second look.
`bench/openings_probcut2.txt` remains as a valid fresh set, used once by the
failed gate.

### Process

The working agreement is unchanged: one issue at a time, red/green TDD with a
RED stop, append-only PR notes, and the review contract. Tuning uses equal
node budgets on the tune and validation sets, with sweep values and
acceptance rules fixed before any games are run. Each feature lands behind a
`SearchConfig` switch and is enabled in `standard()` only after its own
strength gate passes. Suggested order: Multi-ProbCut, the learned evaluation,
endgame and search speed, then parallel search; the opening book is optional.

### Interruptible long-running jobs

The machine that runs training, calibration and matches can be reclaimed at
any moment. Every tools job expected to run longer than 10 minutes (data
generation, fitting, tuning sweeps, validation matches, asset builds) must
meet these requirements. A job that doesn't is not done.

1. **Stoppable at any instant.** SIGINT, SIGTERM, SIGKILL or power loss must
   never corrupt or invalidate completed work. This holds by construction, not
   through signal handlers: every finished unit is already flushed (item 5),
   so the default termination loses only the units in progress. No handler is
   required. The tools crate stays `std`-only with no `unsafe`.
2. **Restart by re-running the same command.** The job finds its own output,
   verifies it, skips completed units and continues. No manual bookkeeping,
   no editing of files, no separate resume procedure.
3. **Bounded loss.** An interruption loses at most the units in progress: one
   per worker. A unit is small, normally at most a few minutes of compute
   (one match pair, one calibration position, one self-play game).
4. **Same result.** A resumed job's output is identical to an uninterrupted
   run's whenever the job is deterministic: equal-depth or equal-node work,
   seeded sampling, fitting. Units and their seeds derive from stable IDs, not
   from schedule, wall clock or completion order.
   - Equal-time matches can't be bit-identical. Their pre-registered rules
     (job count, load-average voiding) apply to each resumed chunk, and a
     resumed run uses exactly the same pre-registered pairs.
5. **Durable, verifiable output.**
   - Output is append-only and line-atomic: one complete line per finished
     unit, flushed when written.
   - Each file starts with a header recording the full configuration: command,
     seeds, budgets, input file hashes, engine commit.
   - On restart a truncated trailing line is discarded and redone.
   - A header that doesn't match the requested configuration is a hard error,
     never a silent mix.
6. **Visible progress.** Units done and total, elapsed time and an estimate,
   at least once a minute, so a stall is distinguishable from slow progress.
7. **Tested.** Each such job has a test that interrupts it mid-run (SIGKILL in
   the middle of a unit), restarts it, and checks the final output equals an
   uninterrupted run's. It also checks that a mismatched header is rejected
   and a truncated last line is redone.
8. **Isolated checkout.** A long run executes from a dedicated git worktree
   pinned at the commit under test (`git worktree add <dir> <commit>`), never
   from the development checkout. Development keeps committing, and every
   commit changes the engine commit recorded in the header, so a run in the
   development checkout couldn't be resumed. The worktree also keeps
   `cargo build` from replacing the binary mid-run. Journals and logs are
   written inside the worktree and copied into the development checkout
   when the run is done. Remove the worktree only after that copy.

Analysis steps that only read finished output (reports, fits over recorded
data) must be cheap to re-run from scratch. They need no checkpointing.

---

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Pass/terminal root handling drifts | `search` requires a legal root move; callers pass or end the game; agent returns `None` only when no placement exists |
| Deeper heuristic TT values change fixed-depth results | Reuse heuristic entries only at equal remaining depth; deeper entries order only; deep-then-shallow test |
| Held-out openings overlap groups | Freeze position sets with canonical D4 keys; group by canonical position after ply 4; reject overlap; keep an untouched final set |
| TT conflates domain and exactness | Separate `Domain` from `Bound`; reuse endgame bounds; never mix domains |
| Frozen weights still drift | Tools freeze the complete evaluator function; pass evaluator config into search; version artifacts for search+eval claims |
| Tools harness state leaks or invalid actions pass | Per-game agent factories; separate agent/bootstrap RNG streams; absolute side-to-move; release action validation |
| Bootstrap intervals are degenerate | Finite-sample conservative fallback; withhold claims; define zero/one/all-equal cases and Elo endpoints |
| Feature gains are not isolated | Compare with the preceding accepted config; ablate; predeclare selection; untouched final validation |
| Size budget is optimistic | `<=999` engine+CLI metric, separate 1,300-line tools budget and 1,200-line trainer budget; Phase 2 vertical slice; cumulative counts every phase; early cuts |
| Phase gates depend on future work | Stage score tests with real dependencies; move solver timeout to Phase 6; local abort-store rule |
| Deadline tests are flaky | Deterministic expired-deadline/node-limit tests; tolerant wall-clock smoke; define clock start and overrun |
| DESIGN and PLAN conflict | Mark DESIGN superseded and point to these contracts |
| Performance claims are unverifiable | Frozen corpus, hardware/build context, warm/cold TT, completion and latency distributions |
| Full-key Zobrist collision | Store the full 64-bit key; index collisions are rejected; full-key aliases are accepted as negligible residual risk; validate stored moves |
| CLI/input bugs | Validate before indexing, handle EOF, bound depth before `u8` conversion, define seed zero |

## Definition of done

Production issues:

- The change is in the named production module, not in tests or bench data.
- New behavior has a test under `tests/` or `tools/tests/` that fails without the change.
- `cargo test`, `cargo test --manifest-path tools/Cargo.toml`, and
  `scripts/check.sh` pass.
- The issue records commands, seeds/IDs, evaluator config, depth, node budget
  or time control, and results for measurements.
- The cumulative engine and tools LOC are recorded and within their ceilings.

Documentation, test-only, scaffold, and measurement issues use the checks
appropriate to them:

- Docs: links resolve, claims match current code/plan, no stale numbers.
- Tests: the test fails for the intended reason and is deterministic where
  claimed.
- Scaffold: builds, policy checks pass, budget guard reports.
- Measurement: corpus/config/hardware recorded, raw results retained, and the
  gate updated with evidence.
