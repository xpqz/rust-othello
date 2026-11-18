# Time management: Design

Nov 18, 2026 · Draft for review · SKIS epic #151, design issue #160

> Scope: a per-game clock, from which the engine allocates the time for
> each move. The tools match driver gets a clock control for a fair gate.
> With a flat per-move budget, every search, agent and caller behaves bit
> for bit as today. `docs/PLAN.md` stays the source of truth for budgets,
> statistics and process. One decision belongs to the user and is marked
> **User decision**; the calls marked *(my call)* are proposals for review.

## Why

The engine thinks for a flat 3 s per move (`src/cli.rs:155`). At that
setting, over the 400 side-games of the committed #142 match
(`docs/tuning/threshold_142_match.journal`, two `standard()` builds at
3,000 ms):

| per side, per game | mean | median | 90th pct | max |
|---|---|---|---|---|
| total thinking time | 52.8 s | 52.0 s | 55.3 s | 63.4 s |

The longest single move was 12.0 s. That is the exact solve's allowance,
`SOLVE_ALLOWANCE` × 3 s (`src/search.rs:139`, `:556`). A flat budget
spends the same 3 s on an opening move, a quiet midgame move and a move the
solver will settle anyway. It never uses the time left over. A clock lets
the engine put more time into the midgame.

## What exists

- `SearchLimits { max_depth, time_ms, max_nodes }` (`src/search.rs:150`)
  - `time_ms` is a deadline from the moment the search is entered.
  - Iterative deepening (`:594-611`) starts depth d+1 whenever the
    deadline has not passed (`stop_iteration`, `:250`). An iteration that
    cannot finish is abandoned, and its time is lost.
  - Inside the search the deadline is checked every 1,024 nodes (`:247`).
- **Exact solve** (empties ≤ the threshold, 18 in `standard()`):
  - its deadline is `SOLVE_ALLOWANCE` (4) × `time_ms`;
  - if the WDL pass does not finish, `fallback_search` (`:621-645`) runs a
    depth-8 heuristic search with `NO_DEADLINE`, and with no node limit
    when `max_nodes` is 0.
- `Agent::choose(board)` has no clock (`src/agents.rs`). `SearchAgent`
  uses fixed limits.
- **Tools match driver** (`tools/src/main.rs`):
  - plays colour-swapped pairs in-process through `harness::play_game`,
    which does not time anything;
  - voids a game only for the `time` control when load exceeds the
    maximum (`void`, `:143`);
  - reads saved results by position after ` points ` (`parse_saved`,
    `:121`).
  - No per-game budget or forfeit exists anywhere.

## Engine

### The iteration start limit

`SearchLimits` gains `start_ms`. No iteration starts once `start_ms` has
elapsed, but an iteration already running continues until `time_ms`.

- `SearchLimits::new` and `SearchLimits::depth` set `start_ms = time_ms`.
  The start check then compares against the same instant as today's
  `stop_iteration`, so flat play is unchanged. The exact-solve allowance
  and `fallback_search` are untouched.
- The 10 full `SearchLimits { … }` literals in `tests/search.rs` gain
  `start_ms` equal to their `time_ms`.
- The check is the same `stop_iteration` test before every iteration,
  depth 1 included, against `start_ms` instead of `time_ms`. As today, a
  limit already passed before depth 1 returns the fallback result.
- **Why:** under a clock, a search that starts iteration d+1 at 0.6 of its
  allotment usually needs several times the elapsed time to finish it. The
  start limit stops it beginning work it will throw away. The hard limit
  `time_ms` still bounds the move.

### Allocation

`pub fn allot(board, remaining_ms, threshold) -> SearchLimits` in
`src/search.rs`, where `R` is the remaining clock, `e` the empties and
`T` the endgame threshold:

- **Exact solve, `e ≤ T`:** `time_ms = start_ms = R / 8`. The solve's
  deadline is then 4 × R / 8 = R / 2.
- **Heuristic, `e > T`:**
  - `m = (e − T + 1) / 2` is the number of own moves before the solve;
  - `w = 3/2` when `T < e ≤ 44`, else `1`;
  - target `t = R · w / (m + 4)`;
  - `start_ms = t` and `time_ms = min(2t, R / 4)`.

  The `+4` keeps a reserve for the solve. Because `R` is re-read every
  move, unused time flows into later moves.
- `max_depth` is 64, `max_nodes` is 0, and every millisecond value is at
  least 1.
- **Constants (my call, fixed before the gate):** 8, 3/2, 44, 4, 2 and 1/4.
  They are not tuned. The gate is one look at this one package. At a
  clock of about 52 s (the gate's expected C), the first move gets about
  2.1 s, a move at 40 empties with 35 s left about 3.5 s, and the solve at
  18 empties with 16 s left 2 s (deadline 8 s), against the flat player's
  3 s everywhere (12 s for the solve).

### Worst case per move

- A heuristic move ends by `time_ms` ≤ R / 4, plus at most 1,024 nodes
  between clock checks.
- A solving move ends its WDL and exact passes by R / 2. If the WDL pass
  times out, the existing unbounded depth-8 fallback follows, from at most
  18 empties.
- Every move therefore leaves at least half the clock, less any fallback
  time. A flag needs a fallback that outlasts R / 2. That is not expected,
  but it is counted and reported.

### The agent hook

- `Agent` gains `fn clock(&mut self, _remaining_ms: u64) {}`. The default
  does nothing, so every existing agent is unchanged.
- `clock_agent(config)` builds a `SearchAgent` that uses
  `allot(board, remaining, config.endgame_threshold)` once `clock` has been
  called, and its stored limits otherwise. `search_agent` is unchanged.
- Book moves go through `book::play` as before and cost no clock time.
- **Estimated size:** 25–35 engine lines (PLAN: 20–30), against 204 lines
  of headroom (1,296 of 1,500).

## Tools: the clock control

- **A clock belongs to an agent.** A time field `c<ms>` (for example
  `pc:80:c53000`) builds `clock_agent` with a game clock of `<ms>` for that
  side. A flat agent (`pc:80:3000`) has no clock and plays exactly as
  today.
- **Timing:** the driver times every `choose` in-process, with `Instant`
  around the call, for both sides, and records each side's total.
  - For a clocked side it subtracts the elapsed time and calls
    `agent.clock(remaining)` before each move.
  - A clocked side whose clock goes below zero has flagged and loses the
    game: 0 points, the opponent 1, whatever the discs. Unclocked sides
    cannot flag.
- **Control:** `control()` returns `clock` when either agent is clocked,
  and `void()` treats `clock` like `time`. A game played above
  `--max-load` is void and replayed, flagged or not, so machine load
  cannot decide a flag.
- **Agent names:**
  - an optional `+book` suffix on any search agent sets `book: true`, so
    the #156 gate needs no second tools change;
  - `pc:80` is `ProbCut::FITTED` (t = 80), so `pc:80:3000` is
    `standard()` at 3 s with the book off;
  - existing names parse as before.
- **Journal:**
  - lines gain `black_ms <ms> white_ms <ms> flag <none|black|white>`
    after `points … load …`, where `parse_saved` reads by position;
  - the agent names, and with them any clock, are already in the header,
    and a mismatch is a hard error.
  - Resume, voiding and replay are unchanged. The interrupt-and-restart
    test covers a clocked match.
- **User decision (Nov 18): raise the tools budget**, chosen over cutting
  existing tools code or moving the driver to the trainer. *(My call)*:
  the clock control is an estimated 60–90 tools lines against 8 left at
  1,292 of 1,300, so I propose a ceiling of 1,400. It is raised in the
  tools issue, with `tools/tests/budget.rs`, `AGENTS.md` and
  `docs/PLAN.md` kept consistent.

## Gate (pre-registered in the gate issue before any game)

- **Equal total time:** the candidate's whole game clock equals the time
  today's flat player actually spends per game, not the flat player's
  ceiling. A 90 s clock for both would hand the candidate the ~37 s per
  game that the flat player leaves unused. A win could then come from
  extra time rather than better allocation, which is not the question the
  epic asks.
- **Baseline:** `pc:80:3000`, today's player, unchanged and unclocked.
- **Candidate:** `pc:80:c<C>`, the clock allocator with a clock of C.
  - **C**, the baseline's mean thinking time per side per game, is
    measured on the current build. It is taken from a pilot of 50 pairs of
    `pc:80:3000` against itself on `bench/openings_tune.txt`, under the
    gate's `--jobs 4 --max-load 6`, about 45 min. C is that mean rounded
    down to a whole second. The #142 evidence above predicts about 52 s.
  - The pilot's journal and C are committed in the gate issue before any
    gate game. Nothing is chosen after a gate game.
  - The candidate cannot exceed C. The baseline's own usage is reported,
    so the equal-time claim can be checked after the run.
- **Book:** off on both sides.
- **Openings:** a fresh set, `bench/openings_time.txt`, generated by
  `gen_openings` with canonical end positions disjoint from every existing
  set (`openings_book.txt` included). It is frozen in the gate issue before
  any game.
- **One look:** 400 pairs, seed fixed in the issue, `--jobs 4 --max-load 6`,
  in a pinned worktree. About 800 games × 2 × 53 s ÷ 4 jobs ≈ 6 h. It is
  interruptible: re-running resumes from the journal.
- **Rule:** enable the clock allocator for play if the lower end of the
  95% pair-bootstrap interval of the candidate's score (one-sided 2.5%,
  10,000 resamples) is above 0.500, under the repository's degeneracy
  policy. Otherwise it stays off and the result is recorded. No other
  constants or clocks are tried on this set.
- **Reported, not gated:**
  - thinking time per side per game (mean, max), the baseline's mean
    against C;
  - flags;
  - voided and replayed games;
  - depth reached by game stage, for each side.
- **If enabled:** CLI play and the GUI move from a flat 3 s per move to a
  clock of C per game, the same average total, in a follow-up issue.

## Tests

### Engine (RED, in the engine issue)

- `SearchLimits::new` and `::depth` give `start_ms == time_ms`. The
  existing search suite stays green unchanged, apart from the literal
  fields.
- **Start limit:** `start_ms = 0`, with a long `time_ms`, starts no
  iteration and returns the fallback result (depth 0), as `time_ms = 0`
  does today. With `start_ms = time_ms` and a depth limit, results equal
  `SearchLimits::depth`.
- **`allot`:**
  - the solving branch has `time_ms = start_ms = R / 8`;
  - in the heuristic branch, `start_ms ≤ time_ms ≤ R / 4`;
  - it is non-decreasing in `R`;
  - midgame weight: the same `R` and `m` give 3/2 the target inside
    `(T, 44]`;
  - every value is at least 1 ms, also for `R` = 0 and 1.
- **Agents:** `clock` is a no-op on existing agents (`search_agent`
  results unchanged). A `clock_agent` given a clock plays the exact
  solver's move in a small endgame. Book hits still answer with
  `book: true`.

### Tools (RED, in the tools issue)

- A deliberately slow clocked test agent flags: 0 points, `flag`
  recorded, the opponent gets the win. An unclocked agent never flags,
  and both sides' times are recorded.
- A flagged game with load above the maximum is void and replayed.
- The journal fields parse, a different clock in an agent name is refused,
  and a clocked match survives SIGKILL and resumes with identical
  results.
- Name grammar:
  - `c<ms>` builds a clocked agent, and `c0` or a non-number is refused;
  - `+book` sets `book: true` on any search kind and is refused on
    `random` and `greedy`;
  - existing names parse as before.

## Issues (created after approval)

1. **Engine clock allocation:** `start_ms`, `allot`, `Agent::clock`,
   `clock_agent`. Red/green.
2. **Tools clock control:** clocked agents, timing, flagging, the journal fields, the
   name grammar, and the budget raise. Red/green.
3. **Clock gate:** generate and freeze `openings_time.txt`, run the pilot
   for C, pre-register, run, report. Run work.
4. #156 (book gate) then uses the clock control. Its clock settings are
   pre-registered there.

## Not in scope

- Increments, byoyomi, or pondering on the opponent's time.
- Extending a search when the best move changes, or other in-search time
  heuristics beyond the start limit.
- Tuning the allocation constants. A later issue can tune them on the tune
  set if the gate passes and evidence suggests it.
- Changing `fallback_search` or the exact-solve allowance.
