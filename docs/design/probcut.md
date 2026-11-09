# ProbCut: Design

Nov 9, 2026 · Draft, revision 2

> Scope: selective pruning in the heuristic (midgame) search. The exact
> endgame solver is untouched. `docs/PLAN.md` remains the source of truth for
> budgets, the statistics policy and process. Revision 2 answers the first
> review; [Review response](#review-response) maps each finding to its change.

## Summary

ProbCut uses a cheap shallow search to predict an expensive deep search, and
skips the deep search when the prediction says, with stated confidence, that
it would fail high or fail low. Multi-ProbCut (MPC) applies the same test at
every depth, with stage-dependent parameters.

The first version (v1) is deliberately conservative:

- **Full-width verification probes.** ProbCut is disabled inside its own
  probes, and probes read only full-width TT entries. Runtime probes are
  therefore the same predictor the calibration measures.
- **A selective TT domain.** Entries written by selective nodes are marked
  `Domain::Selective`. Full-width probes never consume them, and the weaker
  meaning of those entries is stated, not hidden.
- **The fitted model in full.** The cut thresholds use the fitted slope `a`
  and intercept `b`, with conservative integer rounding. The doc no longer
  assumes `a = 1, b = 0`.
- **A supported domain.** ProbCut runs only at `(depth, empties)` cells the
  calibration covers with enough held-out evidence. Everything else stays
  full-width.
- **Calibration on cut decisions.** The fit is judged on held-out false-high
  and false-low rates for the decisions ProbCut would actually make, not on
  `R²`.
- **A binding enablement rule.** It is pre-registered and covers equal nodes,
  equal time and inconclusive results, with one look at the validation set.

Recursive probes, the static-evaluation prefilter and multiple checks per
node are later candidates, each calibrated and gated on its own.

Estimated engine cost: 50-70 formatted lines, within the Phase 2 budget (see
[Budget](#budget)).

## Background

Buro (1995): for a fixed evaluator, a depth-`d` value `v_d` is predicted by a
depth-`d'` value `v_s`:

```
v_d = a · v_s + b + e,     e ~ N(0, σ²)   (model, to be validated)
```

At a node with window `(α, β)`, the deep search fails high with model
probability at least `Φ(t)` when

```
v_s ≥ H(β) = (β + t·σ − b) / a
```

and fails low with the same probability when

```
v_s ≤ L(α) = (α − t·σ − b) / a
```

Each test is one null-window search of depth `d'` around `H` or `L`. On
success the node returns `β` (or `α`) without the deep search.

Multi-ProbCut (Buro 1997) applies this at every depth `d ≥ d_min`, with a
shallow depth per depth and parameters per game stage. It optionally runs
several `(d', t)` checks per node.

**Edax is a reference, not a template.** Its σ model is a quadratic
transform of a linear combination of empties, depth and shallow depth
(`eval_sigma` in `src/eval.c`). Its `search_probcut` (`src/midgame.c`):

- skips PV nodes;
- prefilters each check with the static evaluation;
- searches its probes with ProbCut still enabled, capped at two nested levels;
- varies the threshold `t` by selectivity level.

Its performance therefore says little about this simpler v1. This engine's own
measurements must establish its value.

## Where it fits

### Current shape of `negamax`

1. Count the node, check the stop conditions.
2. Pass or terminal handling.
3. Depth 0 returns the static evaluation (heuristic mode).
4. TT probe and cutoff.
5. Move ordering.
6. Move loop, then TT store.

`ctx.exact` switches the same function into the exact solver.

### Search modes

`Ctx` gains one flag, `probing: bool`, next to `exact`. Together they give
three modes:

| Mode | When | ProbCut | TT reads (heuristic) | TT writes |
|---|---|---|---|---|
| Full-width | `probcut: None`, or inside a probe (`probing`) | off | `Heuristic` only | `Heuristic` |
| Selective | `probcut: Some(_)` and not probing | on | `Heuristic` or `Selective` | `Selective` |
| Exact | `exact` | off | `Endgame` only (unchanged) | `Endgame` |

With `probcut: None`, no node is ever selective, so behaviour is bit for bit
today's. This is test 1.

### Insertion point

The check runs **between steps 4 and 5**:

- **after the TT probe**, which is cheaper than any probe;
- **before move ordering**, which a cut makes unnecessary;
- **only in selective mode** (`!exact && !probing` and `probcut` set);
- **only inside the supported domain** (see below).

`root()` does not run the check itself. **Root children can still cut**,
because they are ordinary `negamax` nodes. What the engine promises is a legal
move from the last *completed selective* iteration. It does not promise that
every root child was searched full-width.

### The check (v1)

Sketch; names indicative:

```rust
#[derive(Clone, Copy, Debug)]
pub struct ProbCut {
    t_centi: u32,        // threshold in hundredths of σ, validated 50..=400
    a_milli: i32,        // slope in thousandths, validated 500..=1500
    b: i32,              // intercept, centidiscs
    sigma: SigmaModel,   // chosen by the fit, see below
    depths: (u32, u32),  // supported remaining depth, inclusive
    empties: (u32, u32), // supported empties, inclusive
}

impl ProbCut {
    pub fn new(...) -> Result<Self, String>  // the only constructor; validates ranges
}
```

The check, in selective mode, at a node of remaining depth `d` with `e`
empties:

1. If `(d, e)` is outside `depths × empties`, there is no check.
2. Compute `d' = shallow_depth(d)` and `σ = max(1, sigma(d, d', e))`.
3. Compute the margin `m = ceil(t_centi · σ / 100)` in `i64`.
4. Compute the high threshold `H = ceil((β + m − b) · 1000 / a_milli)` in
   `i64`. Rounding toward the harder-to-reach side keeps the check
   conservative.
5. If `H ≤ EVAL_BOUND`, run a **full-width** probe: set `probing`, call
   `negamax(board, d', H − 1, H, ctx)`, restore `probing`. If the result is
   `≥ H`, return `β`.
6. Compute the low threshold `L = floor((α − m − b) · 1000 / a_milli)`.
7. If `L ≥ −EVAL_BOUND`, probe with window `(L, L + 1)`. If the result is
   `≤ L`, return `α`.
8. Otherwise fall through to the normal move loop.

Notes:

- **Arithmetic.** All threshold arithmetic is `i64`. `t_centi`, `a_milli`
  and the σ model are range-checked by `ProbCut::new`, and σ is clamped to at
  least 1 throughout the supported domain. `INF`-sized windows produce `H` or
  `L` beyond `±EVAL_BOUND`, so the guard skips them. No `as` cast can turn a
  large setting into a negative margin, because the fields are validated
  before any cast.
- **Guards on transformed thresholds.** The skips compare `H` and `L`, not
  `β ± m`, so they stay correct for any accepted `a` and `b`. No heuristic or
  terminal value lies outside `±EVAL_BOUND`, so a probe against a threshold
  beyond it cannot succeed.
- **Return values.** A cut returns `β` or `α`, a probabilistic bound. It is
  not a fail-soft guarantee: the fail-soft contract holds for the full-width
  and exact modes, and selective results are deliberately approximate. The
  docs and `SearchResult` documentation say so.
- **Cancellation.** A probe returns `None` on a stop, which propagates through
  `?` as it does today. `probing` is restored on every exit path, which a test
  pins.
- **Instrumentation.** `Ctx` gains `probcut_tries` and `probcut_cuts`
  counters (high and low each). Tests use them to cover branches
  deterministically, and `bench` can report them.

### Shallow depth

```rust
fn shallow_depth(d: u32) -> u32 { 2 * (d / 4) + (d & 1) }
```

| d  | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 |
|----|---|---|---|---|---|---|---|----|----|----|
| d' | 1 | 2 | 3 | 2 | 3 | 4 | 5 | 4  | 5  | 6  |

`d − d'` is even, so **in the absence of passes** both searches place their
horizon with the same side to move. Passes cost no depth, so a pass changes
the side to move at the horizon. The fit measures that effect, and nothing
here relies on parity holding exactly. The calibration also compares
`d' = d − 2` and `d' = d − 4` (see [Fitting](#fitting)).

### The σ model

The fit chooses among three candidates, by the held-out decision criterion
below and then by line cost:

1. **Linear:** `σ = S0 + S_GAP·(d − d') + S_EMPTY·e` (3 constants).
2. **Edax-style:** `s = A·e + B·d + C·d'`, then `σ = q2·s² + q1·s + q0`
   (6 constants).
3. **Stage table:** one `(S0, S_GAP)` pair per supported stage bucket (up to
   8 constants).

Each is stored in `SigmaModel` with integer coefficients (σ in centidiscs).

Asymmetry: underestimating σ is the dangerous direction. The fit accepts a
model only if, in every supported cell, the model σ is at least **95%** of the
held-out empirical σ. There is no symmetric tolerance, and overestimates are
acceptable.

### The supported domain

ProbCut runs only where calibration supports it:

- **Depth:** `d_min = 3` up to the deepest calibrated depth (proposed 12).
  Nodes with more remaining depth search full-width. In a depth-16 iteration,
  the top plies run full-width and ProbCut acts below them.
- **Empties:** the calibrated range, proposed **18-52** initially:
  - The upper end matches the positions the 8-ply opening sets produce. The
    first few moves of a game, with more empties, run full-width.
  - The lower end stays clear of the near-terminal region, where heuristic
    horizons start reaching exact outcomes and the error structure changes.
- **Evidence floor:** a `(depth, stage)` cell is supported only if the
  held-out set holds at least **200 fired decisions** in each direction for
  it. Cells below the floor are excluded.

The range widens only through a new calibration run that adds evidence for
the new cells. A linear formula producing a number outside the sample is not
evidence.

### TT provenance policy

- **Selective nodes write `Domain::Selective`.** That includes parents that
  consumed a cut child and stored a value, even `Bound::Exact`. Such a value
  means "the selective search's depth-`d` result", which is approximate.
- **Full-width nodes write `Domain::Heuristic`.** These are probes and
  ProbCut-off search, and their entries are genuine full-width results.
  Inside a probe, every node is full-width and reads only `Heuristic`
  entries, so nothing selective enters a probe's value.
- **Selective nodes may read both domains.** A full-width result is at least
  as good as a selective one.
- **Move hints** may come from either domain. Hints affect ordering and node
  counts, not values.
- **Replacement** stays always-replace. Selective and full-width entries
  compete for slots, a performance effect the node benchmark captures.
- **Endgame** entries are unchanged. The exact solver reads and writes only
  `Domain::Endgame`, so the 14-empty guarantee is independent of all this.
- **Code shape.** `Entry::cutoff` takes the set of acceptable heuristic
  domains: `Heuristic` in full-width mode, both in selective mode. The store
  site picks the domain from the mode. `Entry` stays 16 bytes, because
  `Domain` already exists.

## Configuration and API

- `SearchConfig` gains `pub probcut: Option<ProbCut>`. `None` gives today's
  search. `ProbCut::new` is the only constructor, so invalid parameters cannot
  be built.
- `PROBCUT_FIT` is a `const` holding the fitted parameters, with a provenance
  comment (fit log, commit). `standard()` keeps `probcut: None` until the
  enablement rule passes. Turning it on is then a separate one-line commit.
- `SearchResult`: `depth` reports the nominal depth of the last completed
  iteration, documented as selective when ProbCut is on. `status` keeps its
  meaning.
- **Struct literals:** every `SearchConfig { … }` literal in `tests/` and
  `tools/` (about ten in `tests/search.rs`, plus `phase2_config`) gets
  `probcut: None`. None may use `..SearchConfig::standard()`, which would
  silently pick up the switch later.
- The frozen `phase2` baseline stays full-width.

## Fitting

Calibration, tuning and validation are long-running jobs. They must meet
`docs/PLAN.md` § Interruptible long-running jobs: they are stoppable at any
instant, resume by re-running the same command, lose at most the units in
progress, and give identical results for deterministic work.

The fitting code lives in `tools/examples/fit_probcut.rs`. It is uncounted
data-generation code (decision 045ea9b) and writes `docs/tuning/probcut_fit.log`.

### Positions

All positions come from games started from `bench/openings_tune.txt`. The
validation and final sets are never touched. There are two sources:

1. **Line positions:** positions from self-play games of the standard engine
   with ProbCut off.
2. **Variation positions:** interior nodes visited during depth-8 to depth-10
   full-width searches of line positions. A seeded reservoir sampler collects
   them, together with the `(α, β)` window each node was searched with. These
   cover the searched alternatives that ProbCut actually meets, not just
   principal lines, and supply realistic windows for the decision measurement.

Positions are deduplicated by Zobrist key across both sources. Each position
keeps its game id.

### Split

The split is by game: 70% calibration-fit and 30% held-out, with a seeded
assignment. All positions from one game, including variation positions, fall
on one side. This stops near-duplicate positions within a game from leaking
across the split.

### Measurements

For every position, use **one fresh table per position, separate from any
selective test or match table**, with ProbCut off. Run full-window
`negamax(board, d, −INF, INF, ctx)` for `d = 1..=12` in ascending order. The
public `Ctx`/`set_table`/`negamax` API needs no engine change. Strict
equal-depth reuse keeps each recorded `v_d` a genuine full-width depth-`d`
value.

Sample sizes are 4,000 positions for `d ≤ 10` and 1,000 for `d = 11-12`,
before the split. Deep cells stay thinner, so the evidence floor decides
whether they are supported.

### Fit (calibration-fit split)

For each cell `(d, d'(d), stage bucket)`, with stage buckets every 6 empties
from 18 to 52:

1. Ordinary least squares `v_d ~ a·v_s + b`, then residual σ, recorded per
   cell. The engine uses one global `(a, b)` unless the per-cell fits differ
   materially. In that case `a` and `b` become per-bucket constants, still
   inside `ProbCut`.
2. Fit each σ-model candidate to the cell σs.

### Decision calibration (held-out split)

Fit statistics are reported, but acceptance rests on simulating the actual
decisions on held-out variation positions with their recorded windows:

- A **high cut fires** when `v_s ≥ H(β)` with the model's thresholds and
  rounding. It is a **false high** when `v_d < β`.
- A **low cut fires** when `v_s ≤ L(α)`. It is a **false low** when
  `v_d > α`.
- For each candidate `t` in the sweep, each direction and each cell, report:
  - the firing rate;
  - the false-cut rate among fired decisions, with a one-sided 95%
    Clopper-Pearson upper bound;
  - the nominal model rate `1 − Φ(t)`.
- **Model acceptance (pre-registered):** at `t = 1.5` (nominal 6.7%), every
  supported cell's false-high and false-low upper bounds are at most **12%**.
  Cells that fail are dropped from the supported domain. A model that leaves
  no cells is rejected.
- **Tails are descriptive.** The fraction of residuals beyond 2σ and 3σ is
  reported, but no claim rests on it. With about 200 decisions in a cell,
  even zero observed errors leaves a 95% upper bound of about 1.5%, and
  thinner cells are weaker still.

The fit also reports the `d'` comparison: the decision error rates and the
probe cost (probe nodes divided by deep nodes) for each rule. It keeps the
`2·⌊d/4⌋ + (d mod 2)` rule unless an alternative gives lower false-cut rates
at no greater cost.

## Enablement rule (pre-registered)

All values below are fixed in the RED notes of the tuning issue before any
games are played.

1. **Tune `t` at equal nodes, on the tune set.** Sweep
   `t ∈ {1.0, 1.4, 1.8, 2.2}` against the full-width standard engine, at the
   node budget the full-width engine reaches in about 1 s, with 200 pairs per
   value. Pick the best point estimate.
2. **One look at validation.** Only the picked `t` is played on
   `openings_validation`. It passes only if **all** of these hold:
   - **Equal nodes:** 200 pairs, Elo lower bound > 0.
   - **Equal time, binding:** 500 ms per move, 200 pairs, Elo lower bound > 0.
     If the interval straddles 0 with a positive point estimate, it is
     extended **once**, to 400 pairs, as pre-registered. If it still
     straddles 0, the candidate fails.
   - **Playing-strength check:** 3 s per move, 50 pairs. The point estimate
     must be ≥ 0. This catches a time-control reversal the 500 ms gate cannot
     see.
3. **On failure:**
   - `standard()` stays off, and the result and reasons are recorded.
   - No other `t` from the sweep is then tried on validation.
   - Any later attempt is a new issue with a changed model or design. Its
     notes declare that the validation set has been seen before, and that
     attempt's results are labelled as a second look.
   - The final set stays untouched; it belongs to the Phase 2 exit check.
4. **Reported alongside, not gated:**
   - mean and median depth reached at 3 s from the first 50 validation
     openings, with ProbCut on and off;
   - ProbCut try and cut counts per move;
   - exact-solve completion and timing at 14 empties, which must be unchanged.

Node counts weigh TT hits, leaves and ordered interior nodes alike. That is
why the equal-time gate is binding and not just a confirmation.

## Tests (RED first)

All tests go under `tests/`. **Full-width oracle values come from their own
tables**, never shared with a selective run, so a cached full-width value
cannot satisfy a probe and mask what ProbCut did. The deterministic tests
inject parameters through `ProbCut::new`, using small σ and narrow supported
domains, so that each branch is forced.

### Deterministic branch coverage

1. **Off-switch identity.** With `probcut: None`, `corpus_frozen` and every
   existing search test pass unchanged, and no `Selective` entry is ever
   written.
2. **Exact mode does unchanged work.** For corpus endgames and some
   13-14-empty positions, the solved score and node count are identical with
   `probcut` set and with `None`, and the try counter stays at 0. Identical
   nodes shows unchanged search work. The counter shows the check never ran.
3. **High cut.** For a midgame position `P` with a known full-width
   depth-`d'` value `v'` (oracle table), use parameters `a = 1.000`, `b = 0`,
   minimum σ and minimum `t`, with the supported depths limited to exactly
   `d`. A window whose `H ≤ v'` returns exactly `β`, with one high try and
   one high cut.
4. **Low cut.** The mirror of test 3: `L ≥ v'` returns exactly `α`, with one
   low cut.
5. **Unsuccessful probes.** A window with `H > v'` and `L < v'` gives one try
   in each direction and no cut. Because the domain is limited to depth `d`,
   so no child can cut, the result equals the full-width depth-`d` value.
6. **Domain bypass.** Below the minimum depth, outside the supported depths,
   and outside the supported empties, the try counter stays at 0.
7. **Cancellation inside a probe.** A node budget that runs out mid-probe
   makes the search return `None` from that node. The iteration reports the
   documented `Partial` or `Fallback` status, `probing` is false afterwards,
   and no entry is stored for the aborted node.
8. **Arithmetic boundaries.**
   - `ProbCut::new` rejects `t`, `a` and σ coefficients outside their ranges.
   - At the extremes of the accepted ranges, with windows at `±EVAL_BOUND`
     and `±INF`, the search runs without overflow (debug build, overflow
     checks on), and the guards skip as specified.
   - The rounding direction of `H` and `L` is pinned on hand-computed cases
     with `a ≠ 1` and `b ≠ 0`.
9. **TT provenance.**
   - (a) A selective search writes only `Selective` heuristic entries, and
     full-width probes write only `Heuristic` ones.
   - (b) A planted `Selective` entry with a wrong value is ignored by a
     full-width search of that position.
   - (c) A selective search does use a planted `Heuristic` entry.
   - (d) A selective search never reads or writes `Endgame` entries in
     heuristic mode (unchanged).
10. **Root contract.** With ProbCut on, the returned move is legal and comes
    from the last completed iteration. The existing depth, time and node-limit
    status tests are re-run with the switch on.

### Empirical quality checks (not correctness guarantees)

11. **Decision error on the corpus.** At depth 8 with `PROBCUT_FIT` at
    `t = 1.5`, the fraction of selective results on the wrong side of
    full-width null-window bounds is below a pre-registered ceiling (proposed
    15%). This is a regression tripwire for the fitted constants, not a
    bound guarantee.
12. **Node reduction.** At fixed depth 8 over the corpus midgames, the
    selective node count is strictly below the full-width one. The size of
    the reduction is reported as a benchmark against a 50% target and is not
    asserted. A smaller reduction can still be a strength gain.

### Tools

13. **Fitting example unit test.** Least squares on synthetic
    `v_d = 0.9·v_s + 20 + N(0, 100²)` recovers `a`, `b` and σ within
    tolerance. The decision simulator counts false highs and false lows
    correctly on a hand-built table.
14. **Agent kind.** If a `pc:<t>` agent kind is added to
    `othello-tools match`, it gets parsing tests matching `ab:<d>`.

**Expected RED failures:**
- Tests 2-14 fail to compile until `ProbCut`, the mode flag, the counters and
  `Domain::Selective` exist.
- After that, 3-9 fail until the check and the provenance policy are
  implemented.
- Test 1 passes at RED by construction and is listed so the reviewer sees it
  run.

## Budget

- **Engine:** about 50-70 formatted lines:
  - `ProbCut` with its validating constructor;
  - `SigmaModel`;
  - `shallow_depth`;
  - the check and the threshold arithmetic;
  - the `probing` flag and the counters;
  - `Domain::Selective` and the domain-aware `cutoff`;
  - the store-site domain choice;
  - `PROBCUT_FIT`.

  From 998 that reaches about 1,060 of 1,499.
- **Tools crate:** 639 of 1,000. A `pc:<t>` agent kind costs about 5 lines.
  Fitting and decision simulation live in `tools/examples`, uncounted.

## Risks

- **Tactical misses.** Calibration on variation positions and per-direction
  decision rates reduce the risk, and the gates measure the net effect.
- **Evaluator coupling.** The fit belongs to this evaluator. Any evaluator
  change requires a refit, and `PROBCUT_FIT`'s provenance comment makes that
  visible.
- **Depth reporting.** Selective depth is not comparable to full-width depth.
  `bench`, the README and ladder logs label it.
- **Frozen comparisons.** `ab:<d>` agents built from `standard()` change once
  the switch flips. Ladder logs state which side had ProbCut, as #108 did for
  the threshold.
- **Time management.** Selective iterations grow unevenly, so more work may
  be discarded at the deadline. The depth report measures this.

## Later candidates (each separately calibrated and gated)

- **Recursive probes:** ProbCut enabled inside probes, optionally with a
  nesting cap. This changes the predictor, so it needs its own decision
  calibration (with the probes searched recursively, as at runtime) as well as
  a strength gate. A cap is an accuracy decision, not just an overhead one.
- **Static-evaluation prefilter:** skip a probe when the static evaluation
  makes success implausible, as Edax does. This saves probe cost, and the
  calibration must show it does not change the decisions.
- **Multiple checks per node:** a cheaper `d'' < d'` with a larger `t₂`,
  tried first.
- **Widening the supported domain:** new cells, once new calibration runs
  supply the evidence.

## Proposed issue breakdown

1. **Calibrate ProbCut** (tools/examples and fit log; no engine change).
   Acceptance: per-cell `a`, `b` and σ; the chosen σ model with the 95%
   floor; held-out decision rates with upper bounds; the supported domain;
   the `d'` comparison.
2. **Selective search behind a config switch** (engine). RED: tests 1-12.
   GREEN: the check, the provenance policy and `PROBCUT_FIT` from issue 1;
   `standard()` still off.
3. **Tune `t` at equal nodes** (tools), using the pre-registered sweep on the
   tune set.
4. **Validation gate and enable**: the single validation look under the
   enablement rule. On a pass, the one-line `standard()` change and the doc
   updates. On a failure, a recorded result with the switch left off.

## Review response

| # | Finding | Change |
|---|---|---|
| 1 | Skipping the store at a cut node does not keep the TT full-width | `Domain::Selective` for every selective-node store. Full-width probes read only `Heuristic` entries. The weaker meaning of selective entries is stated ([TT provenance](#tt-provenance-policy)). |
| 2 | Recursive probes are a different predictor | v1 probes are full-width (`probing` flag) and read only full-width entries. Recursion moves to later candidates, needing its own calibration. |
| 3 | `\|a−1\| ≤ 0.1` does not bound the bias | The fitted `a` and `b` are kept in the thresholds, rounded conservatively in `i64`. |
| 4 | Runtime goes beyond the calibrated domain | The supported `(depth, empties)` domain comes from evidence, with an evidence floor. Everything outside it stays full-width. |
| 5 | The fit shows regression quality, not cut reliability | Held-out split by game, variation positions with real windows, deduplication, per-direction false-cut rates with upper bounds, an asymmetric σ floor, tails descriptive only. |
| 6 | The arithmetic is unsafe | Validated constructor, `i64` arithmetic, σ ≥ 1, conservative rounding, guards on `H`/`L`, extreme-value tests. |
| 7 | No binding equal-time rule | Binding 500 ms gate with one pre-registered extension, a 3 s point-estimate check, a single validation look, and an explicit failure procedure. |
| 8 | Missing deterministic branch tests | Tests 3-10 force each branch with injected parameters and counters. Oracles use separate tables. The quality checks are labelled empirical. The exact-mode claim now says "unchanged work". |
| 9 | Incorrect explanatory claims | Parity holds only without passes. Root children can cut. Edax's σ is a quadratic of a linear form, and its prefilter, PV skip and selectivity are listed. |

## References

- M. Buro, "ProbCut: An Effective Selective Extension of the Alpha-Beta
  Algorithm", ICCA Journal 18(2), 1995.
- M. Buro, "Experiments with Multi-ProbCut and a New High-Quality Evaluation
  Function for Othello", NECI Technical Report, 1997.
- Edax source: `src/eval.c` (`eval_sigma`) and `src/midgame.c`
  (`search_probcut`), https://github.com/abulmo/edax-reversi
