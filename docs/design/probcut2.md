# ProbCut v2: Design

Nov 14, 2026 · Draft, revision 2 · SKIS epic #144, design issue #145

> Scope: selective pruning in the heuristic (midgame) search, fitted to the
> learned pattern evaluator. The exact solver is untouched. `docs/PLAN.md`
> stays the source of truth for budgets, statistics and process. This is a
> fresh design. It cites v1 (`docs/design/probcut.md`, tag `probcut-v1`)
> for its evidence, not as a template. Revision 2 answers the first review;
> [Review response](#review-response) maps each finding to its change.

## Why a second version, and what v1 taught

v1 (#109–#118) was fitted to the old hand-written evaluator, used
full-width probes and a separate TT domain, and supported remaining depths
up to about 10. It passed equal nodes (0.5220 [0.5055, 0.5383], 1,000
pairs) and 500 ms (0.5240 [0.5075, 0.5403], 1,000 pairs), then failed a
3 s check of 100 pairs on its point estimate (0.4825 [0.4350, 0.5325]).
That check could not resolve an effect of the size the first two gates
measured; the result is consistent with a small gain, no effect or a small
loss.

Since then:

- **The evaluator changed.** The learned pattern evaluator replaced the
  hand-written one (0.7788 against it). v1's σ and regression constants
  describe the old evaluator and cannot be reused.
- **The engine searches deeper.** Search is about 3–4× faster per node
  (#135–#139). At 3 s per move the engine now completes depth 12–17 on
  corpus positions with 22–44 empties (measured 14 Nov, 11 positions:
  12, 12, 13, 13, 13, 13, 14, 14, 14, 15, 17).
- **ProbCut is the chosen next experiment.** #143 found no improvement in
  two small probes of evaluation cost; that does not show evaluation gains
  are exhausted. Searching fewer nodes is the direction chosen, not one
  forced by a demonstrated floor.

One hypothesis, not a finding: v1's depth cap left the deep interior of 3 s
searches full-width, which would explain a gain at 500 ms that faded at
3 s. v2 covers the depths a 3 s search reaches; it does not depend on the
hypothesis being true.

v1's lessons adopted (`docs/design/probcut.md` § Status): fit after the
evaluator is final; calibrate deep enough; tune `t` over a wider range;
screen node saving at equal depth before matches; make 3 s per move with
about 400 pairs the binding gate.

## Summary of the design

| | v1 | v2 |
|---|---|---|
| Evaluator | hand-written | learned patterns (`pattern::evaluate`) |
| Probes | full-width, `probing` flag | recursive selective (option B, the reviewer's choice), an experiment behind an off switch |
| TT | separate `Domain::Selective` | one heuristic domain; each table bound to one search policy |
| Supported domain | depth 3 to about 10 | a compiled rectangle of (depth, empties), every cell of which passes on its own evidence |
| Calibration positions | game positions | interior nodes of searches from WTHOR roots, deduplicated by canonical position |
| Splits | fit / held-out | fit / selection / audit, by canonical position; audit used once |
| Runtime knob | `ProbCut` struct, 6 validated fields | `probcut: Option<ProbCut>`: the model and `t`, injectable by tools; `ProbCut::FITTED` holds the result |
| Engine lines | about 105 + 128 | about 60 |
| Binding gate | 500 ms, 1,000 pairs, plus a 100-pair 3 s check | 3 s, 400 pairs, one look, fresh openings |

## Probe type: B, recursive selective probes

The reviewer favours B, as an experiment behind the off-by-default switch.
Probes are ordinary `negamax` calls of depth `s`, so nodes inside a probe
run their own checks. Nesting is bounded by depth: each probe is shallower
than its node (`1 ≤ s < d`), and checks run only at `d ≥ d_min`. This is
our own variant: one check per node with one shallow depth, depth-bounded
recursion and no nesting counter. Buro's Multi-ProbCut also considers
several shallow depths per node, and Edax caps probe nesting explicitly;
neither is copied, and this variant needs its own validation, which the
gate provides. Option A (full-width probes with a separate domain) is not
pursued.

## The check

At a heuristic node (not exact) with remaining depth `d` and `e` empties,
after the TT probe and before move ordering, with `p = config.probcut`:

1. Skip unless `p` is `Some`, `p.d_min ≤ d ≤ p.d_max` and
   `p.e_min ≤ e ≤ p.e_max`. Inside that rectangle every cell is supported
   (see [Supported domain](#supported-domain)); there are no holes.
2. `s = shallow(d)`, a fixed formula with `1 ≤ s < d` on the whole
   rectangle (tested at every cell).
3. `σ = clamp((p.s0 + p.s1·e + p.s2·d) / 100, 1, SIGMA_MAX)` in centidiscs,
   coefficients in hundredths of a centidisc; `SIGMA_MAX = 2·EVAL_BOUND`.
4. `t = min(p.t, 400)` hundredths of σ; `m = ceil(t·σ / 100)`.
5. `a = clamp(p.a_milli, 500, 1500)` thousandths, always positive; `b`
   clamped to `±EVAL_BOUND`.
6. High: `H = ceil((β + m − b)·1000 / a)`. If `H ≤ EVAL_BOUND` and
   `negamax(board, s, H − 1, H)` returns `≥ H`, return `β`.
7. Low: `L = floor((α − m − b)·1000 / a)`. If `L ≥ −EVAL_BOUND` and
   `negamax(board, s, L, L + 1)` returns `≤ L`, return `α`.
8. Otherwise continue to the move loop.

Steps 1–7 are one public function, `probcut(board, d, α, β, ctx) ->
Option<Option<i32>>` (outer `None` on cancellation, inner the cut bound),
which `negamax` calls and the runtime replay calls directly.

Steps 3–7 are in `i64`. With the clamps the largest magnitude is about
`(INF + 4·SIGMA_MAX + EVAL_BOUND)·1000 ≈ 1.06·10^9`, far inside `i64`.
Ceiling and floor of the signed division use `div_euclid` with a
correction, so rounding is toward the harder-to-reach side for any sign.
Every `ProbCut` value, including one built directly with out-of-range
fields, therefore gives defined, conservative behaviour; RED tests cover
the boundaries. Cancellation propagates through `?`.

`shallow(d)` is chosen by the fit among `2·⌊d/4⌋ + (d mod 2)` (v1's rule,
valid from `d = 3`), `d − 4` (valid from `d = 5`) and `d − 6` (from
`d = 7`). A rule is a candidate only on rectangles where it is valid, so
the rectangle's `d_min` respects it.

A node that does not run a check is not thereby full-width: below a
selective node, descendants check, and any node may read TT entries a
selective search wrote. "Full-width" describes a whole search run with
`probcut: None` on a table that has only ever served that policy.

## Configuration, API and table lifecycle

- `SearchConfig` gains `pub probcut: Option<ProbCut>`. `ProbCut` is a
  small `Copy` struct with public fields `t`, `s0`, `s1`, `s2`, `a_milli`,
  `b`, `d_min`, `d_max`, `e_min`, `e_max`. Tools inject candidate models
  and `t` values without recompiling.
- `ProbCut::FITTED` is a `const` with the chosen model and `t`, with a
  provenance comment (fit log, commit). `standard()` keeps `probcut: None`
  until the gate passes; enabling it is a separate one-line commit.
- Frozen baselines (`tools/src/baselines.rs`, `phase2` and `classic`) set
  `probcut: None` explicitly, so they never inherit a future enabled
  `standard()`.
- **Table lifecycle, the precondition for a single domain.** A table serves
  one search policy: one evaluator, endgame threshold, ordering and
  `probcut` value (on or off, model, `t`). Every caller that changes the
  policy clears the table or uses a new one:
  - match agents: `SearchAgent` builds its own table and its configuration
    is fixed for the agent's life (already so);
  - CLI: one table per process with a fixed configuration (already so);
  - GUI: clears its table whenever a setting that enters `SearchConfig`
    changes, before the next search;
  - tools (`probcut_fit`, benches): a fresh or cleared table per query or
    policy, stated in each tool.
  Under this precondition a full-width search only ever sees full-width
  entries, so `probcut: None` is bit for bit today's search, and cached
  bounds always mean what the current policy means.
- `SearchResult::depth` is the nominal depth; with ProbCut on it is a
  selective depth, which the doc comment says.
- `Ctx` gains counters of ProbCut tries and of high and low cuts, for RED
  tests, the runtime replay and bench reporting.
- Exact scores stay protected by `Domain::Endgame`. Identical exact node
  counts or times in games do not follow, because earlier play and table
  contents differ; only the solver's results at a given position are
  invariant.

## Calibration data

**What exists (#146, running).** `probcut_fit collect`: unit `n` picks a
WTHOR game and a root at 26–58 empties, runs a plain depth-8 alpha-beta
search from it with the engine's move ordering, and keeps one visited node
with remaining depth ≥ 3 and at least 16 empties by reservoir sampling. Its
record holds the stable unit id, game index, board, empties, the deepest
depth `D`, node count, time, and the full-width values `v_0 … v_D` (`v_0`
static; `v_d` from `search_tt` at depth `d`, `endgame_threshold` 0, one
table per worker cleared once per node and kept across that node's depths).
The header binds depths, seed, sampling constants, the WTHOR file set,
commit and binary hash. 6,000 units, `D = 14`, every 8th unit `D = 16`.

**Completeness.** `search_tt` with `SearchLimits::depth(d)` has no time or
node limit, so every recorded value is a completed depth-`d` search; `fit`
rejects a record whose value count is not `D + 1`.

**Not recorded, and not needed.** The sampler's remaining depth and window
are not used: each node supplies every `(d, s)` pair up to its `D`, and
decisions are evaluated with defined boundary windows (below), not with the
windows of the depth-8 sampler, which are unrelated to the depths being
modelled.

**Deduplicated and split by position.** `fit` keys each record by the
canonical (D4) form of its board. Records whose key appeared at a lower
unit id are dropped. The split comes from a hash of the key, not the game:
60% fit, 20% selection, 20% audit. Exact and symmetric duplicates therefore
never sit on both sides. The `split` field written by `collect` is ignored.

## Decision checks

Two checks, on positions the fit never saw, with the reference value `v_d`
the completed full-width search.

**Proxy check (full-width predictor), for model selection.** For a cell
(depth `d`, empties bucket) and a model, a pair `(v_s, v_d)` is a boundary
error high if `v_d < a·v_s + b − t·σ`, and low if `v_d > a·v_s + b + t·σ`:
the rate at which a cut at a window exactly on the threshold would be
wrong. It is a proxy for in-search decisions, whose windows vary, and is
labelled so.

**Runtime replay (selective predictor).** For an audit position and a
depth `d` in the rectangle, with the candidate `ProbCut` injected:

- High: `β = v_d + 1`, so the true value is just below the window and any
  high cut is wrong. Call the engine's check directly,
  `probcut(board, d, β − 1, β, ctx)` (public, the same function `negamax`
  calls; it returns the cut bound or nothing), and count a false high if it
  cuts.
- Low: `α = v_d − 1`, window `(α, α + 1)`; a false low if it cuts.

Each query uses a fresh table, never a collection table, so full-width
oracle values cannot satisfy selective probes. Replay is cold and labelled
so: it does not reproduce the warm in-game table, whose contents depend on
the game. It needs the engine check, so it runs after the engine issue's
GREEN, with candidate models injected.

## Supported domain

- **Evidence unit:** one cell is one (depth `d`, empties bucket) pair. A
  position contributes at most one pair per cell, so pairs within a cell
  are independent; pairs of one position across depths are correlated and
  are never pooled into one binomial count.
- **Cell acceptance (selection split):** at least 60 positions; model σ at
  least 95% of the cell's empirical σ; proxy boundary error rate at
  `t = 1.5` with 95% Wilson upper bound at most 15% in each direction
  (6.7% expected under the Gaussian model).
- **Rectangle:** the rectangle of consecutive depths and consecutive
  empties buckets with the most cells, every cell accepted. Its empties
  bounds are the buckets' bounds; `e_min` is also at least
  `endgame_threshold + 2`, and `e_max` at most 56, where the data end.
- **Per-depth reporting:** every cell's counts, σ and rates are printed,
  accepted or not.
- **Deep cells:** tier 2 gives about 750 positions at depths 15–16, about
  150 per bucket on the selection split before deduplication. A cell with
  fewer than 60 is outside the rectangle. If the rectangle ends at depth 14,
  a second collection of depth-16 units (a separate journal,
  `--deep-every 1`, a new seed) may be run before the model is frozen;
  that decision is recorded in #146 before the audit split is opened.

## σ model and model selection

Model (Buro 1995): `v_d = a·v_s + b + ε`, `ε ~ N(0, σ²)`.

- **Forms:** (1) `a = 1, b = 0`, `σ = S0 + S1·e + S2·d`; (2) the same with
  one pooled positive `a` and `b`. Both fit `ProbCut`'s fields; cells are
  checked one by one.
- **Fit split:** coefficients by weighted least squares on per-cell σ.
- **Selection split:** each (rule, form) is inflated by the smallest factor
  that meets the 95% σ floor in every cell of its rectangle; the one with
  the most cells, then the smallest mean σ, is chosen. The selection split
  is model-selection data and no confidence claim is made from it.
- **Audit split, once:** the frozen model's per-cell σ floor and proxy
  rates are recomputed on the audit split and reported with Wilson bounds.
  A cell that fails the floor there is removed by shrinking the rectangle
  (the rule stated here, before the audit); no refit follows.

## Screening and tuning `t`

Candidates, fixed now: `t ∈ {0.8, 1.2, 1.6, 2.0}`. The runtime replay
reports each candidate's false-high and false-low rates on the audit
positions; they are reported, not gated, because they differ by `t` by
construction. The 1.5 proxy check is a model-quality filter only.

1. **Screen every candidate** before any match:
   - node saving at equal depth: the `search_bench` midgame workload
     (depth 10, split fixed at 14 since #143) uses at most 60% of the
     full-width nodes;
   - depth: the median depth completed at 3 s, on the positions after 20
     plies of the first 50 tune openings' recorded games, is at least one
     ply deeper than full-width.
   Candidates failing either are dropped. If none survives, the epic stops
   for discussion and no match is run.
2. **Tune the survivors** on the tune set at 500 ms per move, `pc:<t>`
   against `pc:off`, 200 pairs each, fixed seed. Pick the highest point
   estimate; ties within 0.005 go to the larger `t`. 500 ms is for cost;
   v1's reversal between 500 ms and 3 s is why the binding gate is at 3 s.
3. The picked `t` and model are frozen into `ProbCut::FITTED` before the
   gate.

## Gate (pre-registered in the gate issue before any game)

- **Openings:** a fresh set, `bench/openings_probcut3.txt`, generated by
  `gen_openings` with canonical end positions disjoint from every existing
  set (`openings_probcut2.txt` was used by a ProbCut gate and is spent).
- **One look:** `pc:<t>:3000` against `pc:off:3000`, both otherwise
  `standard()` (threshold 18), 400 pairs, seed fixed in the issue,
  `--jobs 4 --max-load 6`, in a pinned worktree. About 6.5 h. No
  extension.
- **Rule:** enable if the lower end of the 95% pair-bootstrap interval of
  the score (one-sided 2.5%) is above 0.500, under the repository's
  degeneracy policy (an uninformative interval cannot enable). Otherwise
  the switch stays off and the result is recorded; no other `t` or model is
  tried on this set.
- **Reported, not gated:** depth reached at 3 s with and without ProbCut;
  ProbCut tries and cuts per move; thinking time per side.

## Engine budget

The engine is at 1,129 production lines. User decision, 14 Nov: "raise core
budget to 1500"; `tests/budget.rs` has a single guard at 1,500 and the
former 1,150 working guard is gone (epic #144's text updated to match). The
check, `ProbCut` and its constant, and the counters come to about 60 lines.
The budget does not change the design: the domain, validation and table
invariants stay as written. If the gate fails, the code is removed as v1's
was.

## Tests (RED, in the engine issue)

- Off-switch identity: with `probcut: None` and fresh tables, identical
  node counts and values to today on the golden workloads
  (`tools/tests/golden.rs`, `tests/search.rs`).
- Policy transitions: a table used with ProbCut on, then cleared, gives
  the full-width result exactly; the GUI clears its table when a setting
  changes (render-and-change test in the style of #142's).
- Branch coverage on constructed positions: high cut, low cut, no cut,
  outside the rectangle at each of its four bounds, threshold beyond
  `±EVAL_BOUND`, no check in exact mode, cancellation inside a probe.
- Arithmetic boundaries: `t` above 400, `a_milli` outside 500–1500, extreme
  `s0..s2` and `b`, `α` or `β` at `±INF`: no overflow, and ceiling and
  floor checked against exact rational values for negative and positive
  numerators.
- `shallow(d)` satisfies `1 ≤ s < d` on the whole `ProbCut::FITTED`
  rectangle.
- With ProbCut on, every search returns a legal move under every limit;
  exact solver results unchanged at 14 and 18 empties.
- Fit tool (#146): fits and floor on synthetic data, deduplication and the
  canonical split, rectangle selection on a constructed grid.

## Issues

1. #146: calibration data and `fit` (running; `fit` revised for the
   splits, deduplication, rectangle and audit above).
2. Engine check behind the switch, `ProbCut`, counters, GUI table
   lifecycle, `pc:` agents, explicitly-off baselines.
3. Runtime replay, screen and tuning of `t` (pre-registered).
4. Fresh opening set and the 3 s gate (pre-registered); enable or record.

## Timeline

About 3 h of implementation with reviews after approval, the calibration
(running, finishing about 17:10 on 14 Nov), about 3 h of screening and
tuning, and about 6.5 h of gate, an overnight run.

## Review response

Review of `bf8fa02` with `70eb618`:

1. **Decision replay.** The record already holds board, unit and game id
   and full-width values; completeness is now stated. The proxy check uses
   defined boundary windows and is labelled a proxy; the runtime replay
   runs the engine's own check on injected models with boundary windows
   `β = v_d + 1`, `α = v_d − 1`, cold fresh tables, never the collection
   tables, after the engine exists. See [Decision checks](#decision-checks).
2. **Supported domain.** One evidence unit, the (depth, bucket) cell; no
   pooling across depths; Wilson bounds per cell; a compiled rectangle with
   four bounds, `e_max` included, no holes; three splits by canonical
   position with deduplication; the audit split used once. See [Supported
   domain](#supported-domain) and [σ model](#σ-model-and-model-selection).
3. **TT lifecycle.** One table per policy, cleared on any change, with each
   caller named; off-switch identity stated under that precondition;
   baselines explicitly off; "no check here" distinguished from
   "full-width". See [Configuration](#configuration-api-and-table-lifecycle).
4. **Order.** Screen all candidates first, tune survivors, fixed tie and
   none-survive rules; the 1.5 check is a model filter, per-candidate rates
   reported. See [Screening](#screening-and-tuning-t).
5. **Gate.** One look at 400 pairs, no extension, model and `t` frozen
   before it. See [Gate](#gate-pre-registered-in-the-gate-issue-before-any-game).
6. **Additional.** `1 ≤ s < d` enforced per rule and tested; positive `a`
   in thousandths, conservative signed division, clamps on every field;
   #143's correction carried into the motivation; epic #144 updated;
   `tests/budget.rs` formatting fixed.

## References

- M. Buro, "ProbCut: An Effective Selective Extension of the Alpha-Beta
  Algorithm", ICCA Journal 18(2), 1995.
- M. Buro, "Experiments with Multi-ProbCut and a New High-Quality
  Evaluation Function for Othello", 1997.
- Edax 4.7 `src/midgame.c` (`search_probcut`) and `src/eval.c`
  (`eval_sigma`), read as a reference only; not vendored (#130).
- `docs/design/probcut.md` and tag `probcut-v1`.
