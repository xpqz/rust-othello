# Time management: Design

Nov 18, 2026 · Revision 2, after the first review · SKIS epic #151, design issue #160

> Scope: a per-game clock, from which the engine allocates the time for
> each move. The tools match driver gets a clock control for a fair gate.
> With a flat per-move budget, every search, agent and caller behaves bit
> for bit as today. `docs/PLAN.md` stays the source of truth for budgets,
> statistics and process. One decision belongs to the user and is marked
> **User decision**; the calls marked *(my call)* are proposals for review.
> [Review response](#review-response) maps each finding of the first review
> to its change.

## Why

The engine thinks for a flat 3 s per move (`src/cli.rs:155`). The nearest
evidence is the committed #142 match
(`docs/tuning/threshold_142_match.journal`): 100 pairs at 3,000 ms per
move between two older builds, with endgame thresholds 18 and 14. Pooled
over its 400 side-games:

| per side, per game | mean | median | 90th pct | max |
|---|---|---|---|---|
| total thinking time | 52.8 s | 52.0 s | 55.3 s | 63.4 s |

The threshold-18 side averaged 51.5 s and the threshold-14 side 54.2 s.
This is only an approximation of today's player. The gate measures the
current build in a pilot (below). The longest observed move was 12.0 s,
the exact solve's deadline of `SOLVE_ALLOWANCE` × 3 s
(`src/search.rs:139`, `:556`). That is an observed maximum, not a bound. A flat budget
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

`SearchLimits` gains `start_ms`. The check before each iteration, depth 1
included, compares the elapsed time with `start_ms` instead of `time_ms`.
An iteration that has started runs until it finishes or until the
in-search deadline `time_ms`, whichever comes first.

- `SearchLimits::new` and `SearchLimits::depth` set `start_ms = time_ms`.
  The check then compares against today's instant, so flat play is bit
  for bit unchanged. As today, a limit already passed before depth 1
  returns the fallback result.
- **Exact branch** (empties ≤ threshold): `start_ms` is not read. The
  solve keeps today's deadline of `SOLVE_ALLOWANCE` × `time_ms`, and
  `fallback_search` is unchanged, flat or clocked.
- The 10 full `SearchLimits { … }` literals in `tests/search.rs` gain
  `start_ms` equal to their `time_ms`.
- **Why:** under a clock, an iteration started late usually cannot finish
  before the deadline, and its work is thrown away. The start limit stops
  it beginning.

### Allocation

`pub fn allot(board, remaining_ms: u64, threshold: u32) -> SearchLimits`
in `src/search.rs`. All arithmetic is in `u64` milliseconds, divisions
round down, and products are formed before division (`R` < 2^40 in any
use, so nothing overflows). Here `R` is `remaining_ms`, `e` the empties
and `T` the threshold.

- **Exact branch, `e ≤ T`:** `time_ms = start_ms = max(1, R / 8)`. The
  solve's deadline is then `4 × time_ms`: R / 2 when R ≥ 8 ms.
- **Heuristic branch, `e > T`:**
  - `m = (e − T + 1) / 2` is the number of own moves before the solve;
  - `w = 3/2` when `T < e ≤ 44`, else `1`;
  - target `t = R · w / (m + 4)`, computed as `3R / (2(m + 4))` or
    `R / (m + 4)`;
  - in-search deadline `time_ms = max(1, min(2t, R / 4))`;
  - start cutoff `start_ms = max(1, min(t, time_ms))`.

  The start cutoff never exceeds the deadline (at e = 19, R = 1,000 the
  target is 300 ms, and the start cutoff is clamped to the 250 ms
  deadline). The `+4` keeps a reserve for the solve. Because `R` is
  re-read every move, unused time flows into later moves.
- **Tiny clocks:** below 4 ms of remaining time, the 1 ms minimum exceeds
  R / 4. A search always gets at least 1 ms, and near zero a flag is then
  likely. That is the clock's outcome, counted as below.
- `max_depth` is 64 and `max_nodes` 0.
- **Constants (my call, fixed before the gate):** 8, 3/2, 44, 4, 2 and 1/4.
  They are not tuned. The gate is one look at this one package.
- **Examples** at a clock of about 52 s (the gate's expected C):

  | position | R | start cutoff | in-search deadline |
  |---|---|---|---|
  | first move, e = 60 | 52 s | 2.08 s | 4.16 s |
  | e = 40 | 35 s | 3.50 s | 7.00 s |
  | solve, e = 18 | 16 s | – | 2 s × 4 = 8 s |

  The flat player has 3 s for both start and deadline, and 12 s for the
  solve.

### Deadlines, not bounds

`time_ms` and the solve allowance are deadline settings, not limits on
elapsed time.

- The search polls the clock every 1,024 nodes.
- The exact branch's fallback has no deadline.
- The process can be descheduled.

The engine therefore promises no maximum per move. The clock contract
below is return-and-forfeit: a move is timed after it returns, and an
overrun is charged and counted. Nothing in the engine is bounded
differently for clocked play.

### The agent hook

- `Agent` gains `fn clock(&mut self, _remaining_ms: u64) {}`. The default
  does nothing, so every existing agent is unchanged.
- `clock_agent(config)` builds a `SearchAgent` that uses
  `allot(board, remaining, config.endgame_threshold)` once `clock` has been
  called, and its stored limits otherwise. `search_agent` is unchanged.
- Book moves go through `book::play` as before.
- **Estimated size:** 30–40 engine lines (PLAN: 20–30), against 204 lines
  of headroom (1,296 of 1,500).

## Tools: the clock control

### Clock contract (return-and-forfeit)

- **A clock belongs to an agent.** The time field `c<ms>` (for example
  `pc:80:c53000`) builds `clock_agent` with a game clock of `<ms>`. A flat
  agent (`pc:80:3000`) has no clock and plays exactly as today.
- **What is charged:** the harness times every `choose` call, for both
  sides, from just before the call until it returns. Book hits, lookup and
  call overhead are charged. A pass does not call `choose` and costs
  nothing. Opening moves are not charged.
- **Accounting:** elapsed time accumulates as a `Duration`, with no
  rounding. Before each move, a clocked side receives
  `clock(remaining)`, where `remaining = C − used` in whole milliseconds,
  rounded down. A positive remainder below 1 ms is passed as 0, and
  `allot` then gives its 1 ms minimum.
- **Flag:** a clocked side flags when, after a `choose` returns, `used`
  exceeds `C` strictly. `used = C` is not a flag.
  - The flagging move is discarded, not applied.
  - The game stops there, and the flagging side scores 0 points, the
    opponent 1.
  - That move's elapsed time is still charged and reported.
  - An unclocked side never flags. Its time is recorded only.
- **Testability:** the harness takes its time source as a parameter: a
  closure returning the elapsed `Duration` of each call. Production
  passes a real `Instant` timer, and tests pass scripted durations. That
  makes cumulative debits, the exact boundary, the discarded move, passes
  and book hits deterministic.

### Pairs, load, voiding and resumption

- **Today:** load is sampled once, after both games of a pair. A pair
  above `--max-load` is discarded and replayed, in up to ten passes, after
  which the run stops with an error and re-running resumes.
- **Clocked control:** `control()` returns `clock` when either agent is
  clocked. `void()` treats `clock` like `time`. The rule is the same for
  every pair, flagged or not: one sample after both games, and the whole
  pair is void above the maximum. A flag cannot survive a void pair.
- **Void count:** the driver prints a line to stderr for every discarded
  pair, with its pair number and load. The gate run appends stderr to its
  committed run log across restarts, and void and replay totals are
  counted from that log. The journal itself keeps only accepted pairs, as
  today.
- **Resumption:**
  - complete accepted pairs in the journal are kept;
  - partial or void pairs are replayed with the same openings and agent
    seeds;
  - the agent names, and with them the clocks, are in the journal header,
    and a changed clock is a header mismatch and refused;
  - the report is reproduced from the completed journal alone.
  - Real-time games are not expected to replay identically. The
    identical-results requirement is tested on a deterministic fixture
    with injected elapsed times and load.
- **Journal line:** after `points … load …`, where `parse_saved` reads by
  position, each game adds
  `black_ms <ms> white_ms <ms> flag <none|black|white>`.
  - The `*_ms` fields are each side's charged time, rounded down to whole
    ms, expired move included.
  - `points` carries the flag result. The disc score in the game log is
    the disc difference of the position where the game stopped, kept
    separately from `points`.
  - `moves` lists the moves played, without the discarded one.

### Agent-name grammar

- `c<ms>` may appear wherever a time field may (`phase2`, `classic`,
  `pc`, `ab`), with `ms` ≥ 1. Like a time field, it sets depth 64.
- It is refused with `--nodes`, as a time field is, and refused after
  `classic:d<N>`, whose depth field takes the same position.
- An optional `+book` suffix ends the name, after all fields, on any
  search kind. It sets `book: true`, so the #156 gate needs no second
  tools change. It is refused on `random` and `greedy`.
- `pc:80` is `ProbCut::FITTED` (t = 80), so `pc:80:3000` is `standard()`
  at 3 s with the book off. Existing names parse as before.

### Budget

- **User decision (Nov 18): raise the tools budget**, chosen over cutting
  existing tools code or moving the driver to the trainer.
- *(My call)*: the clock control is an estimated 70–100 tools lines,
  against 8 left at 1,292 of 1,300. I propose a ceiling of 1,400, raised
  in the tools issue with `tools/tests/budget.rs`, `AGENTS.md` and
  `docs/PLAN.md` kept consistent.

## Gate (pre-registered in the gate issue before any game)

### Controls

- **Baseline:** `pc:80:3000`, today's player, unchanged and unclocked.
- **Candidate:** `pc:80:c<C>`, the clock allocator with a game clock of C.
- **Book:** off on both sides.
- **C**, the baseline's own mean thinking time per side per game, is
  fixed before any gate game:
  - **Pilot:** `pc:80:3000` against itself, 50 pairs on
    `bench/openings_tune.txt`, seed fixed, `--jobs 4 --max-load 6`. It
    runs in the gate's pinned worktree and build, and the commit and binary
    hash are recorded before the pilot starts.
  - **C** is the mean of the `*_ms` fields over all 200 side-games of the
    accepted pairs, rounded down to a whole second.
  - The pilot's journal, log and C are committed in the gate issue. C is
    never recomputed from gate data.

### Equal-time condition (pre-registered)

C equalizes the candidate's cap with an estimate of the baseline's usage,
not with its realized usage in the gate. Over all 400 accepted gate pairs,
for each pair let `d` = (the candidate's charged ms in both games) −
(the baseline's charged ms in both games), expired moves included.

- **Equal-time holds** if the candidate's mean charged time per game is
  at most 1.05 × the baseline's realized mean per game in the same pairs.
  That is a 5% tolerance for the baseline's drift between pilot and gate.
- **Flags:** a flag ends the game early and lowers the opponent's charged
  time too. Pairs with a flag stay in both the score and the time
  comparison. The flag already counts as a loss for the candidate.
  Flagged games are reported per colour.

### Rule

- **Enable** the clock allocator for play if both hold:
  - the lower end of the 95% pair-bootstrap interval of the candidate's
    score (one-sided 2.5%, 10,000 resamples) is above 0.500, under the
    repository's degeneracy policy;
  - the equal-time condition holds.
- **Score only:** if the score passes but equal-time does not, the result
  is reported as a comparison at the tested controls. No equal-time
  allocation claim is made, and the allocator stays off.
- Otherwise it stays off and the result is recorded. No other constants
  or clocks are tried on this set.

### Run

- **Openings:** a fresh set, `bench/openings_time.txt`, generated by
  `gen_openings` with canonical end positions disjoint from every existing
  set (`openings_book.txt` included). It is frozen in the gate issue before
  the pilot.
- **One look:** 400 pairs, seed fixed in the issue, `--jobs 4 --max-load 6`,
  in a pinned worktree. About 800 games × 2 × 53 s ÷ 4 jobs ≈ 6 h. It is
  interruptible: re-running resumes from the journal.
- **Reported, not gated:**
  - both realized means, and the mean paired difference `d` with its
    pair-bootstrap interval;
  - each side's maximum per game;
  - flags per side and colour;
  - void and replay totals from the run log.
- **Deferred:** depth reached by game stage. `choose` returns only a
  square, so it is not in this gate.
- **If enabled:** CLI play and the GUI move from a flat 3 s per move to a
  clock of C per game, in a follow-up issue.

## Tests

### Engine (RED, in the engine issue)

- `SearchLimits::new` and `::depth` give `start_ms == time_ms`. The
  existing search suite stays green, with only the literal fields added.
- **Start limit, controlled:** an evaluator that sleeps a fixed 20 ms per
  call makes iteration timing predictable (depth 1 ≈ 4 calls).
  - Choose `start_ms` to fall inside iteration 2 and set a `time_ms` far
    beyond it.
  - The search must report depth 2 (`Partial`), with elapsed time past
    `start_ms`: iteration 2 started before the cutoff and finished after
    it.
  - The evaluator's call count must equal a plain `depth(2)` search's
    count: iteration 3 was not started.
  - An implementation that applied `start_ms` inside the search would stop
    iteration 2 and report depth 1.
- **Edge:** `start_ms = 0` with a long `time_ms` returns the fallback
  result (depth 0), as `time_ms = 0` does today.
- **Exact branch:** with `start_ms = 0` and a solvable small endgame, the
  solve still runs, under `SOLVE_ALLOWANCE` × `time_ms`.
- **`allot`:**
  - exact branch: `time_ms = start_ms = max(1, R / 8)`;
  - heuristic branch: `1 ≤ start_ms ≤ time_ms`, and `time_ms ≤ max(1, R / 4)`;
  - the e = 19, R = 1,000 case is clamped (250/250);
  - it is non-decreasing in `R`;
  - the midgame weight gives 3/2 the target;
  - R = 0, 1, 3 and u32::MAX ms give sane values.
- **Agents:**
  - `clock` is a no-op on existing agents;
  - a `clock_agent` given a clock plays the exact solver's move in a small
    endgame;
  - with `book: true` it still answers book hits.

### Tools (RED, in the tools issue)

All of these use scripted elapsed times and an injected load, unless
noted.

- **Debits and flags:**
  - cumulative debits across moves keep fractional time;
  - `used = C` does not flag, and one nanosecond over does;
  - the flagging move is discarded and not in `moves`;
  - the flagging side gets 0 points, for Black and for White;
  - the disc score is kept separately;
  - passes and opening moves cost nothing, and book hits are charged.
- **Clocks:**
  - an unclocked side never flags, and both sides' times are recorded;
  - a clocked agent receives `clock(remaining)` with the rounded-down
    milliseconds.
- **Pairs and voiding:**
  - a pair above the load limit is void whether or not it flagged, and is
    replayed with the same openings and seeds;
  - the stderr void line is printed;
  - ten void passes stop the run.
- **Journal:**
  - the fields parse, and the saved report aggregates them, flags
    included;
  - a changed clock in an agent name is refused as a header mismatch;
  - a clocked match is SIGKILLed and resumed: complete pairs are kept,
    partial pairs are replayed, and with scripted time and load the
    completed journal and report are identical to an uninterrupted run.
- **Grammar:**
  - `c<ms>` on each kind;
  - `c0`, a non-number, `c<ms>` with `--nodes`, and `classic:d4:c…` are
    refused;
  - `+book` works on search kinds and is refused on `random` and
    `greedy`;
  - existing names parse as before.

## Issues (created after approval)

1. **Engine clock allocation:** `start_ms`, `allot`, `Agent::clock`,
   `clock_agent`. Red/green.
2. **Tools clock control:** the harness time source, the clock contract,
   the journal fields, voiding, the grammar and the budget raise.
   Red/green.
3. **Clock gate:** generate and freeze `openings_time.txt`, run the pilot
   for C, pre-register, run, report. Run work.
4. #156 (book gate) then uses the clock control. Its clock settings are
   pre-registered there.

## Not in scope

- Increments, byoyomi, or pondering on the opponent's time.
- Extending a search when the best move changes, or other in-search time
  heuristics beyond the start limit.
- Bounding `fallback_search`, or changing the exact-solve allowance.
- Tuning the allocation constants. A later issue can tune them if the
  gate passes.
- Depth-by-stage reporting.

## Review response

### First review, of `ca317b51`

| Finding | Change |
|---|---|
| Allocation bounds inconsistent (e = 19, R = 1,000: target 300 > deadline 250); 1 ms minimum vs R/4; untestable start rule | Allocation: integer `u64` ms, round down, explicit clamps `start ≤ time`, tiny-clock rule; examples give both start cutoff and deadline. Exact branch ignores `start_ms` and keeps its allowance. Controlled slow-evaluator test that admits iteration 2 before the cutoff, finishes it after, and rejects iteration 3. |
| Clock enforcement and flag semantics | Return-and-forfeit contract: charged time = each `choose` call (book and overhead included, passes and openings free), `Duration` accounting, strict `>` flag, flagging move discarded, sub-ms remainder passed as 0. "Deadlines, not bounds" states that the engine promises no per-move maximum. Injected time source for deterministic tests. |
| Equal-time gate can mislead | Pilot bound to build, openings and seed before measurement; C from complete accepted pairs, never recomputed. Pre-registered equal-time condition (candidate ≤ 1.05 × baseline realized mean), flags included; enable needs both score and equal-time; otherwise a score-only report with no equal-time claim. #142 described as a pooled approximation (51.5 s at threshold 18). |
| Clocked resumption | Load sampled once per pair after both games, whole pair void, same for flagged and unflagged; ten-pass exhaustion stops the run; void totals from a retained stderr log; flag points separate from disc score; stopped games' `moves`; resume keeps accepted pairs and replays partial or void ones with the same seeds; identical results tested with injected time and load; grammar of `c<ms>` with `--nodes`, depth fields and `+book`; depth-by-stage deferred. |
