# ProbCut v2: Design

Nov 14, 2026 · Draft, revision 1 · SKIS epic #144, design issue #145

> Scope: selective pruning in the heuristic (midgame) search, fitted to the
> learned pattern evaluator. The exact solver is untouched. `docs/PLAN.md`
> stays the source of truth for budgets, statistics and process. This is a
> fresh design. It cites v1 (`docs/design/probcut.md`, tag `probcut-v1`)
> for its evidence, not as a template.

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
- **Node cost is near its floor.** #143 found evaluation at half of midgame
  time and already vectorised; the next gain has to come from searching
  fewer nodes.

One hypothesis, not a finding: v1's depth cap left the deep interior of 3 s
searches full-width, which would explain a gain at 500 ms that faded at
3 s. v2 tests this by covering the depths a 3 s search actually reaches; it
does not depend on the hypothesis being true.

v1's lessons that v2 adopts (`docs/design/probcut.md` § Status): fit after
the evaluator is final; calibrate deep enough; tune `t` over a wider range;
require a large node saving at equal depth before spending matches; make
3 s per move with about 400 pairs the binding gate.

## Summary of the design

| | v1 | v2 (proposed) |
|---|---|---|
| Evaluator | hand-written | learned patterns (`pattern::evaluate`) |
| Probes | full-width, `probing` flag | **reviewer's choice**, see [Probe type](#probe-type-the-decision-for-the-reviewer); recommended: recursive selective |
| TT | separate `Domain::Selective` | one heuristic domain (option B) |
| Supported depth | 3 to about 10 | 3 to 16 (no extrapolation beyond) |
| Calibration positions | game positions | interior nodes of real searches, from WTHOR games |
| Calibration cost | about 1–3 h | about 2–3 h on 6 cores, journalled |
| σ model | three candidate families | the simplest form that passes the floor (formula or per-stage table) |
| Runtime knob | `ProbCut` struct, 6 validated fields | `probcut: Option<u32>` (t in hundredths of σ); fitted model as constants |
| Engine lines | about 105 + 128 | about 45 (B) or 65 (A) |
| Binding gate | 500 ms, 1,000 pairs, plus a 100-pair 3 s check | 3 s, 400 pairs, fresh openings |

## The check

At a heuristic node (not exact) with remaining depth `d` and `e` empties,
after the TT probe and before move ordering:

1. Skip unless ProbCut is on, `D_MIN ≤ d ≤ D_MAX` and `E_MIN ≤ e`.
2. `s = shallow(d)`; `σ = sigma(d, e)` in centidiscs (fitted constants,
   at least 1).
3. `m = ceil(t · σ / 100)`.
4. High: `H = β + m` (with fitted slope and intercept, `H = ceil((β + m −
   b) / a)`; see [Fit](#fit)). If `H ≤ EVAL_BOUND` and a null-window
   search `negamax(board, s, H − 1, H)` returns `≥ H`, return `β`.
5. Low: `L = α − m` (or `floor((α − m − b) / a)`). If `L ≥ −EVAL_BOUND` and
   `negamax(board, s, L, L + 1)` returns `≤ L`, return `α`.
6. Otherwise continue to the move loop.

Arithmetic is in `i64` so no window or σ can overflow; thresholds outside
`±EVAL_BOUND` skip their probe, which also skips full-window nodes on the
side where the bound is infinite. Cancellation propagates through `?` as
everywhere else.

`shallow(d)` keeps `d − s` even, so both horizons have the same side to
move in the absence of passes. The default is v1's
`2·⌊d/4⌋ + (d mod 2)` (d = 8 → 4, d = 12 → 6, d = 16 → 8); the fit compares
it with `d − 4` and `d − 6` and the design keeps whichever gives the most
fired decisions at equal held-out error, as one formula.

`E_MIN` is fitted: the lowest empties count at which the held-out error
stays within bounds, and at least `endgame_threshold + 2`, so ProbCut never
acts on the positions just above the solver where heuristic horizons start
to reach the end of the game.

## Probe type: the decision for the reviewer

**Option A: full-width probes (v1's choice).** Inside a probe, ProbCut is
off and only full-width TT entries are read; selective nodes write a
separate domain.

- The runtime predictor is exactly the calibrated one.
- Costs `Ctx::probing`, `Domain::Selective`, domain-aware `cutoff` and
  store: about 20 lines in `tt.rs` and `search.rs` beyond option B.
- Probes are expensive: a depth-8 probe at a depth-16 node is a full-width
  depth-8 search.

**Option B: recursive selective probes (Buro's Multi-ProbCut; Edax does
this, with a nesting cap).** Probes are ordinary `negamax` calls, so nodes
inside them can cut too. One TT domain.

- Probes are much cheaper, which is where Multi-ProbCut's savings come
  from at depth; depth shrinks geometrically through nested probes, so
  nesting is bounded without a counter (`s < d`, and `D_MIN ≥ 3`).
- The runtime predictor is a selective shallow search, while calibration
  measures full-width pairs. To close that gap, the calibration tool's
  held-out check replays the actual runtime decision (selective probe at the
  chosen `t`) against the full-width deep value, and reports false-cut
  rates for that predictor; the gate is on those rates, not on the
  full-width ones.
- Selective values enter the shared TT under the heuristic domain. With
  ProbCut off nothing selective is ever written, so `probcut: None` stays
  bit for bit today's.
- About 45 engine lines.

**Recommendation: B.** It is smaller, it is the form whose strength is
established in practice, and its one weakness (predictor mismatch) is
measured directly by the held-out replay. If the reviewer prefers A, the
rest of this design is unchanged.

## σ model and fit

Model (Buro 1995), per depth pair and stage: `v_d = a · v_s + b + ε`,
`ε ~ N(0, σ²)`, σ depending on `d` and `e`.

- **Form.** The fit tool tries, in order, and keeps the first that passes
  the floor: (1) `a = 1, b = 0` and `σ = S0 + S1·e + S2·d` (3 constants);
  (2) the same with fitted `a`, `b`; (3) a per-stage table of `(S0, S2)`
  over the 12 evaluator stages (24 constants). Fewer constants win; more
  fired decisions break ties.
- **Floor (asymmetric).** In every supported (depth, stage bucket) cell the
  model σ is at least 95% of the held-out empirical σ. Overestimates are
  allowed.
- **Supported domain.** A (depth, stage bucket) cell is supported if the
  held-out split holds at least 200 fired decisions in each direction,
  pooled over depths within the bucket as v1's revised floor did. `D_MAX`
  is the deepest depth with held-out evidence, aimed at 16. No check runs
  at `d > D_MAX`; at 3 s that leaves only the top one to three plies
  full-width.
- **Held-out decision check.** At `t = 1.5`, every supported cell's
  false-high and false-low rates (upper 95% bound) are at most 10%, for the
  runtime predictor of the chosen probe type.

## Calibration data

**Positions.** Roots are WTHOR positions (137,548 games in
`data/wthor/`), at 20–56 empties, one position per game at a uniformly
chosen empties count. From each root, a depth-10 full-width search is run
and one interior node is sampled per root, uniformly over the nodes it
visits with remaining depth ≥ 3 (reservoir sampling), so the sample has the
distribution of nodes a real search checks rather than of game positions.
Split by game: a hash of the game index sends 70% to fit and 30% to
held-out.

**Measurements.** One iterative-deepening run per sampled node, full-width,
ProbCut off, threshold 0 (no exact solve), table cleared per node, records
`v_0 … v_D`. Each run yields every `(d, s)` pair with `d ≤ D`, so one deep
search serves every depth. Two tiers, from the measured cost of iterative
deepening on corpus positions (cumulative time to depth 14: 0.4–7.6 s; to
16: 1.8–105 s):

- **Tier 1:** 12,000 nodes to depth 14, about 1.5 h on 6 cores.
- **Tier 2:** 1,500 of them continued to depth 16, about 1 h on 6 cores.

Exact numbers are fixed after a timing sample of 100 nodes, before the run,
so the whole calibration fits about 3 h.

**Tool.** `tools/examples/probcut_fit.rs` (exempt from the tools budget by
the user's decision of 14 Nov, quoted in #144), in two subcommands:

- `collect`: the journalled, resumable sampling and measurement job, one
  thread per core (`std::thread::scope`, one `Table` of 64 MB per thread),
  one journal line per node (`v_0..v_D`, empties, game index, split). It
  survives SIGKILL and resumes by re-running the same command; the
  interrupt-and-resume test is part of done.
- `fit`: reads the journal, fits the forms above, applies the floor and
  held-out decision check, chooses the domain, and prints the constants as
  Rust source plus a report (per-cell σ, fired decisions, false-cut rates).
  For option B it also replays selective probes on held-out nodes at the
  candidate `t` values.

## TT policy

Option B: one heuristic domain. A cut stores nothing extra; the parent
stores its value as usual, with the usual depth. Move hints are unchanged.
Endgame entries are untouched. Option A: v1's policy (`docs/design/probcut.md`
§ TT provenance policy).

## Configuration and API

- `SearchConfig` gains `pub probcut: Option<u32>`: `t` in hundredths of σ,
  validated `50..=400` where it is parsed (agents and tools). `None` is
  today's search.
- The fitted model is a set of `const`s in `src/search.rs` (or a small
  `src/probcut.rs` if it reads better), with a provenance comment naming
  the fit log and commit.
- `standard()` keeps `probcut: None` until the gate passes; enabling it is a
  separate one-line commit.
- `SearchResult::depth` is the nominal depth; with ProbCut on it is a
  selective depth, which the doc comment says.
- Agents: `pc:<t>` and `pc:<t>:<ms>` in the tools agent parser, as v1 had.

## Tuning `t`

On the tune set, at 500 ms per move (equal time: v1 showed node counts
mislead at equal nodes), `pc:<t>` against `pc:off`, 200 pairs per value,
`t ∈ {0.8, 1.2, 1.6, 2.0}` (v1 chose its lowest value, 1.0, so the range
now extends below it). Pick the best point estimate. About 40 minutes per
value with 5 jobs, 2.5–3 h in all. 500 ms is chosen for cost; v1's
reversal between 500 ms and 3 s is why the binding gate is at 3 s, and a
`t` that only helps at 500 ms fails there. Before any match, a screen:

- **Node saving at equal depth:** at the picked `t`, the `search_bench`
  midgame workload (depth 10, fixed split since #143) must use at most 60%
  of the full-width nodes, and the median depth completed at 3 s on the
  first 50 tune openings' positions must be at least one ply deeper. If the
  screen fails, matches are not run and the epic stops for discussion.

## Gate (pre-registered in the gate issue before any game)

- **Openings:** a fresh set, `bench/openings_probcut3.txt`, generated by
  `gen_openings` with canonical end positions disjoint from every existing
  set (`openings_probcut2.txt` was used by a ProbCut gate and is spent).
- **Binding:** `pc:<t>:3000` against `pc:off:3000`, both otherwise
  `standard()` (threshold 18), 400 pairs, seed fixed in the issue,
  `--jobs 4 --max-load 6`, in a pinned worktree. About 6.5 h.
- **Rule:** enable if the lower end of the 95% pair-bootstrap interval of
  the score is above 0.500. If the interval contains 0.500 with a point
  estimate above 0.500, extend once to 800 pairs on the same set, as
  pre-registered; enable only if the lower end is then above 0.500.
  Otherwise the switch stays off and the result is recorded.
- **Reported, not gated:** depth reached at 3 s with and without ProbCut;
  ProbCut tries and cuts per move; thinking time per side; exact-solve
  behaviour (unchanged by construction).

## Engine budget

The engine is at 1,129 production lines. User decision, 14 Nov: "raise core
budget to 1500"; `tests/budget.rs` now has a single guard at 1,500 and the
former 1,150 working guard is gone. Option B adds about 45 lines, option A
about 65. Lines are still spent only on the check, the constants and the
switch; if the gate fails the code is removed as v1's was.

## Tests (RED, in the implementation issue)

- `probcut: None` gives identical node counts and values to today on the
  golden workloads (`tools/tests/golden.rs`, `tests/search.rs`), bit for
  bit.
- Deterministic branch coverage on constructed positions: high cut, low
  cut, no cut, skip outside the domain, skip for thresholds beyond
  `±EVAL_BOUND`, no check in exact mode, cancellation inside a probe.
- With ProbCut on, every search still returns a legal move under every
  limit, and the exact solver's results are unchanged at 14 and 18
  empties.
- Fit tool: unit tests for the fits and the floor on synthetic data with
  known σ; journal kill-and-resume test.

## Proposed issues after approval

1. ProbCut v2 calibration tool (`probcut_fit collect` and `fit`), with the
   timing sample and the calibration run.
2. Engine check behind the switch, with the fitted constants and the
   `pc:` agents.
3. Screen and tuning of `t` (pre-registered).
4. Fresh opening set and the 3 s gate (pre-registered); enable or record.

## Timeline

Design review, then about 3 h of implementation with reviews, about 3 h
of calibration, about 3 h of tuning and about 6.5 h of gate. The gate is an
overnight run; with a quick design review, tonight is possible, otherwise
tomorrow night.

## References

- M. Buro, "ProbCut: An Effective Selective Extension of the Alpha-Beta
  Algorithm", ICCA Journal 18(2), 1995.
- M. Buro, "Experiments with Multi-ProbCut and a New High-Quality
  Evaluation Function for Othello", 1997.
- Edax 4.7 `src/midgame.c` (`search_probcut`) and `src/eval.c`
  (`eval_sigma`), read as a reference only; not vendored (#130).
- `docs/design/probcut.md` and tag `probcut-v1`.
