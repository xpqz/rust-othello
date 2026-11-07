# Design review (round 3): 7 November 2026

Reviewed `docs/DESIGN.md`, `docs/PLAN.md` (Revision 3, 932 lines), and all 54
SKIS issues: epics #52–60 and tasks #61–105, all open. Nothing has been
implemented yet, so this reviews the design and the execution plan, not code.
This review changes neither the docs nor the issues. The previous review is
kept as `docs/REVIEW-r2.md`.

**Verdict.** Revision 3 addresses all 12 findings from the previous review (one,
the size budget, only partly), and the plan and the issues now agree with each
other. One new correctness defect
must be fixed before Phase 1: the start position has the colours swapped. The
largest remaining risk is the product goal itself. The plan now has a 910-line
budget with 89 lines of headroom, but the harness and CLI scope has grown past
what those budgets can hold. An unspecified solver trigger can also break time
control. Before more work is added, decide what counts toward the 999 lines and
cut accordingly.

Priority levels: **P1** breaks correctness or an acceptance contract. **P2**
should be fixed in the affected phase. **P3** is a simplification or a nit.

## Disposition of the previous review (REVIEW-r2)

| # | Finding | Status | Where it landed |
| --- | --- | --- | --- |
| 1 | Forced-pass root vs `SearchResult` | Addressed | PLAN:114-130; #73, #74, #83 (see new finding 9 for a simpler alternative) |
| 2 | Deeper heuristic TT values | Addressed | Strict equal-depth reuse, PLAN:189-192, 216-217; #87, #88, #89 |
| 3 | Seed-disjoint ≠ position-disjoint | Addressed, with a new defect | Canonical D4 keys, PLAN:616-622; #80 (see new finding 5) |
| 4 | TT domain vs exactness | Addressed | `Domain`/`Bound` split, PLAN:176-201; #87, #90, #91 |
| 5 | Frozen weights ≠ frozen evaluator | Addressed | `SearchConfig.eval`, `baselines.rs`, PLAN:262-266, 612-615; #72, #79 |
| 6 | Harness lifecycle and validation | Addressed | Factories, RNG streams, side-to-move bit, release validation; #77, #81 |
| 7 | Bootstrap degeneracy | Addressed | PLAN:627-640; #78 |
| 8 | Incremental feature selection | Addressed | PLAN:809-823; #99 |
| 9 | Size budget and feasibility gate | Partly addressed | `<=999` metric and Phase 2 slice gate exist; the budget realism problem remains (new finding 3) |
| 10 | Phase gates needing future work | Addressed | Phase 0 pure conversion; solver timeout moved to Phase 6; local abort rule; per-kind DoD |
| 11 | Deadline test assumptions | Addressed, one ambiguity left | PLAN:147-171; #83-85 (see new finding 4) |
| 12 | DESIGN/PLAN conflicts | Addressed | DESIGN has a banner and inline "Superseded" notes; #57 and #89 both call the percentages initial targets |

## New findings

### 1. P1: The initial position has the colours swapped

References: PLAN:485-487. Issues: #53, #66, #70, #75.

The plan puts black on d4/e5 and white on d5/e4 (`me = bit 27 | bit 36`,
`opp = bit 28 | bit 35`, black to move). The official start position is the
other way round: **white on d4/e5, black on d5/e4**. Played from the plan's
constant, black's first moves are `c5 d6 e3 f4` (checked by enumeration), not
the standard `c4 d3 e6 f5`.

None of the planned tests would catch this:

- Perft is unchanged, because the swapped position is a reflection of the real
  one.
- The differential test is blind if the naive reference takes its start
  position from the same constant.
- Symmetry tests are colour-agnostic.

The visible effects: #75's `play accepts d3` is illegal on the first move,
notation does not match published games, and the WTHOR replay planned as a
later extension would be wrong.

**Fix:** use `me = (1<<28)|(1<<35)` and `opp = (1<<27)|(1<<36)`. Add a fixture
asserting that black's opening moves are exactly {c4, d3, e6, f5}. In #70, have
the naive reference build the start position from coordinates on its own, not
from the production constant.

### 2. P1: Where the endgame solver triggers is unspecified, which can break time control

References: PLAN:156-158, 163, 542, 772. Issues: #58, #83, #90.

"Trigger at `empty_count <= endgame_threshold`" does not say whether this
applies at the root only or at any interior node. The two readings behave very
differently:

- **Interior trigger:** a 14-empty depth-1 search solves each 12-empty child
  exactly, and so does a midgame search whose deep lines reach 12 empties. Each
  of those calls is unbounded, the "<1 s median" is per root, and deadline
  behaviour becomes erratic. The depth≤4 minimax oracle (PLAN:360) and the TT
  domain rules would both have to model the switch.
- **Root-only trigger:** this is simple, but the plan should say so. It should
  also say what iterative deepening means at a solver root (is there one
  iteration?).

The threshold is also defined twice: in `SearchLimits` (PLAN:163, 675) and in
`SearchConfig` (PLAN:542). Pick one. The TT lifetime rule depends on which,
because a table belongs to a single threshold.

**Fix:** state that the trigger is root-only. A solver root runs the WDL search
and then the exact search, with no heuristic iterations. Remove one of the two
threshold fields. Update #90 and #83.

### 3. P1 (product): The scope does not fit in 999 lines, and the boundary is undecided

References: PLAN:14-31, 309-325, 591-660. Issues: #65, #75, #77-80, #104.

The budget has grown from 805 lines (Revision 2) to 910 lines, while the
previous review already called 805 optimistic. The modules that grew in scope
kept nearly the same budgets:

| Module | Budget | What it must now contain | Rough readable estimate |
| --- | ---: | --- | ---: |
| `harness.rs` | 120 | Game loop, side-to-move bit, release validation, opening generation, position-set (de)serialisation, canonical D4 keys, overlap/duplicate/length stats, pair scoring, three RNG streams, percentile bootstrap, Hoeffding fallback, five degeneracy cases, Elo with finite endpoints, ordered logs | 200-260 |
| `cli.rs` + `main.rs` | 105 | Argument parsing, printer, exact parser, interactive play with EOF handling, `perft`, `bench` with a corpus loader, `match` with seven flags, agent-name grammar (`random`, `greedy`, `ab:d`, `ab:d:t`, `baseline:phase2[:t]`, `baseline:release-N`) | 150-200 |
| `search.rs` | 170 | Three root cases, fallback, status/solved, iterative deepening, deadline and node limits, ordering, TT integration with domain rules, solver, WDL plus widening | 190-240 |

These estimates are judgement, not measurements. On them, the total lands
around 1,050-1,250 lines. The cut order (PLAN:39-44) cannot recover that much:
parallelism has no budget line, so cutting it saves nothing, and D4 transforms
cannot be cut at all (finding 5).

What gets squeezed is the engine itself. The tooling for measuring strength
statistically competes for the same lines as the code that produces strength.

**Decision needed (the user's call):** what is the "platform" that must be
under 1000 lines?

- **(a) Engine plus a playable CLI ≤999.** Experiment tooling (bootstrap,
  position-set generation, tuning ladder, logs) goes in a separate `tools/`
  crate or an ignored test with its own budget. This is the recommended option.
  It matches the goal ("fast, small, capable") and frees about 150-250 lines
  for search and evaluation.
- **(b) Everything ≤999.** Then simplify hard: one opening source, bootstrap
  only (the fallback just prints "insufficient data"), no ordered logs beyond
  the seed, agent grammar `random|greedy|ab:<d>[:<ms>]|phase2[:<ms>]`, and
  generate position sets offline.

Whichever is chosen, rerun the Phase 2 vertical-slice gate with real counts
before Phase 3 adds anything.

### 4. P2: `SearchStatus::Aborted` conflates "timed out" with "no usable result"

References: PLAN:137-158, 125. Issues: #56, #83, #84.

In timed play nearly every search ends because the deadline fired. If depth 6
completes and depth 7 is cut off, the plan does not say whether the status is
`Complete` or `Aborted`. Callers are told to "check `status` or `depth`", which
is two signals for one fact.

Also, the clock includes "TT clearing" (PLAN:147-149). Clearing a 64 MB table
takes milliseconds. At the short tuning controls the plan recommends, it could
use up the whole budget. The TT contract says the table is cleared between
games, not per search, so this line contradicts it.

**Fix:**

- Status is one of `Complete` (reached `max_depth`, or solved), `Partial`
  (deadline after ≥1 completed depth, result usable), or `Fallback` (no
  completed depth).
- Remove TT clearing from per-search work.
- Pick one fallback rule ("first legal move" or "greedy", PLAN:150) so results
  are deterministic.

### 5. P2: D4 transforms are both "cuttable" and required, and the prefix rule collapses every opening into one split

References: PLAN:39-41, 256-258, 279, 389-390, 603, 616-623, 656. Issues: #69,
#80, #81, #104.

- **D4 contradiction.** Decision 7, the file layout, and cut order item 1 say
  `sym.rs` can leave production. Phase 3 and #80, however, need canonical D4
  keys in `harness.rs`. Either `sym.rs` stays in production and in the budget,
  or position sets are generated and validated offline (an ignored test or a
  tool) and the harness only loads them. The second choice fits option (a) in
  finding 3.
- **Prefix rule.** "Prefix-related openings go to one split", taken literally,
  is transitive. Every opening shares the empty-move prefix, and all four first
  moves are D4-equivalent, so all openings end up in one split. Define the
  grouping key explicitly. One option is the canonical position at ply 4: about
  60 groups (REVIEW-r2 counted 60 distinct 4-ply positions up to D4), which then constrains how many distinct groups each split gets.
- **Two opening sources.** Phase 3 lists both "seeded openings generated once
  per game" (PLAN:603) and frozen `bench/openings_*.txt` (PLAN:616). The gate
  command (PLAN:656) has no `--openings` flag. Choose one source for matches.
  Frozen files are simpler and remove the opening RNG stream.

### 6. P2: Conventions as doc comments consume the line budget

References: PLAN:28-31, 313, 463. Issues: #63, #65.

The metric counts `///` and `//!` lines, `lib.rs` is budgeted at 20 lines, and
#63 asks for about 13 conventions as `src/lib.rs` doc comments. That alone is
30-60 counted lines.

**Fix:** keep the conventions in `//` comments or in README/PLAN, or stop
counting doc comments. The current rule penalises documentation, and documenting
the code is the stated purpose of the project.

### 7. P2: The budget guard can be gamed through rustfmt configuration, and "formatted" is undefined

References: PLAN:18, 398-400. Issues: #65.

A `rustfmt.toml` setting such as `max_width = 200`, `use_small_heuristics =
"Max"` or a large `chain_width` can compress code a lot while still being
"formatted".

**Fix:**

- Forbid `rustfmt.toml`/`.rustfmt.toml`, or pin it to the defaults, and have
  the budget test check this.
- Say whether `tests/budget.rs` runs rustfmt itself or relies on
  `cargo fmt --check` having passed first in `check.sh`. The second option
  means a bare `cargo test` can report a misleading count.
- Define whether `examples/` and `benches/` are "compiled production targets".

"Rejects implementation hidden in tests" cannot be checked mechanically, and
production code cannot depend on tests anyway. Narrow the guard to `include!`,
`#[path]`, `build.rs` with generated code, and `macro_rules!` used as a
compression trick.

### 8. P2: No production PRNG module

References: PLAN:270-287, 455, 549. Issues: #64, #74, #77, #78.

`random_agent(seed)`, opening generation, and the bootstrap all need a seeded
PRNG in production code. Only `tests/common/mod.rs` has one. Name the module
(for example `rng.rs`, about 10 lines), budget it, and move the seed-zero rule
there. The tests should reuse the production generator, not a copy.

### 9. P3: Searching a forced-pass root is machinery with no caller

References: PLAN:114-130. Issues: #73, #74, #77, #83.

The plan specifies three root kinds, a separate timeout rule for forced-pass
roots, and tests of "both `None` kinds" through search, agent, CLI and harness.
Yet every caller already knows the root has no legal move (`legal_moves == 0`)
and only needs to pass.

**Simpler contract:** `search` requires at least one legal move and returns
`best: u8`. The game loop and CLI handle pass and terminal positions themselves.
This removes a branch-heavy root path and a set of tests, and none of them
protect playing strength. If an evaluation display is wanted at a pass, call
`search` on `b.pass()` and negate the result.

### 10. P3: Store the board in the TT, not a Zobrist key

References: PLAN:179-216, 716-720. Issues: #87, #89.

Store `me: u64, opp: u64` (16 bytes) in each entry and index with a multiply
mix of the two, such as `(me ^ opp.rotate_left(17)).wrapping_mul(K) >> shift`.
That removes several things at once:

- the Zobrist tables and the const-fn splitmix64 generator
- per-node recomputation of up to 64 XORs
- the full-key alias risk policy and the separate "oracle mode"
- the `key == 0` sentinel, replaced by an all-zero board, which is impossible
  in a real game

Entries grow from about 16 to about 24 bytes, so a 64 MB table holds 2.8M
entries instead of 4M. That is irrelevant at these depths, and the change likely
saves 15-25 lines and one class of bugs.

`mv: u8` also needs a no-move sentinel (for example 64), because nodes that
fail low store no best move.

### 11. P3: Tune with node budgets, not wall-clock time

References: PLAN:33-37, 816. Issues: #84, #99, #102.

`max_nodes` is already a public limit. Matches with a fixed node budget are
deterministic and reproducible, and they parallelise with no CPU-contention
controls. Optional parallel matches (#102) then become trivial or unnecessary.
Keep a short equal-time run only as the final check that node budgets and wall
time rank configurations the same way.

### 12. P3: Smaller items

- **`EVAL_BOUND == 6400` equals the largest terminal score** (PLAN:62-64). A
  clamped heuristic of +6400 ties a proven 64-0 win, and more generally a large
  heuristic value outranks a small proven win. That is normal for engines of
  this kind. If proven wins should always dominate, clamp heuristics strictly
  inside the terminal range, or offset proven wins.
- **Eval antisymmetry is required** (#72, #96, #98). This rules out a
  side-to-move tempo term or a parity term. That is fine for now, but say it is
  a deliberate choice.
- **Baseline budget is too small.** `baselines.rs ≤40` must hold a complete
  copy of a Phase 2 evaluator budgeted at `≤60`, so the two budgets don't fit
  together.
- **Perft terminal rule.** "A terminal position with depth > 0 counts as one
  node" only matters past depth 10 and is not checked against Aart Bik's
  table. Call it the project's own convention, not his.
- **Endgame target.** The 12-empty median < 1 s is easy for a bitboard solver.
  Exact solves at 12 empties typically take milliseconds. If "capable" matters,
  raise the default threshold based on measured completion rate (for example
  16-18), or make it time-adaptive.
- **Ordering cost.** Ordering by opponent reply count calls `play` plus
  `legal_moves` for every child at every interior node. Restrict it to nodes
  with depth ≥ 2 or so, and use cheap static ordering near the leaves.
- **Statistical power.** A target of "≥55% with the lower bound >50%" on 200
  pairs is roughly break-even: if the true score is 56-57%, the test often
  fails. Use about 400 pairs, or treat a miss as inconclusive rather than a
  failure.
- **Edition.** The plan says `edition 2021`. Edition 2024 has been stable since
  Rust 1.85; use it unless there is a reason not to.
- **Inconsistent units.** #60 states game counts (1000/400/100) while PLAN and
  #100 state pairs. They agree numerically, but should use one unit.
- **Unlisted cut item.** #104's cut order adds "extra eval features/fancy
  ordering", which PLAN:39-44 does not list.

## Worth keeping

- Side-relative `Board` with `play` returning the swapped view. Direction masks
  are correct (checked against the shift table).
- Correctness evidence from independent sources: naive reference, perft with
  Bik's depth 1-10 table, D4 commutation, invariants, and a shallow minimax
  oracle.
- Terminal and pass handling before the depth cutoff, true fail-soft, and
  `Option<i32>` as the only cancellation channel.
- Strict equal-depth TT reuse, plus the deep-then-shallow test.
- The evaluator passed into search from the start, with a fully frozen baseline
  function.
- Paired, colour-swapped games as the sampling unit, with incremental
  comparison against the preceding accepted configuration.

## Recommended order

1. Fix the start position (finding 1) and decide the 999-line boundary
   (finding 3). Both must happen before Phase 0 code, because they change
   #61, #65, #66 and the module map.
2. Settle the root-only solver trigger, the status enum, and the single
   threshold field (findings 2 and 4). Simplify the root contract and the TT
   entry if accepted (findings 9 and 10). Update #73, #83, #87, #90.
3. Resolve D4 and opening-source ownership and the prefix grouping key
   (finding 5), and the PRNG module (finding 8).
4. Fix the budget metric's doc-comment and rustfmt loopholes before Phase 0
   lands `budget.rs` (findings 6 and 7).
5. Then follow the plan as written, with the Phase 2 vertical slice as a hard
   go/no-go on real line counts.
