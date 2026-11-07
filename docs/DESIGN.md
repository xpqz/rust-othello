# Othello Engine in Rust: Design Document

Nov 7, 2026 · @Stefan Kruger

> **Historical design; superseded where noted.** `docs/PLAN.md` (Revision 4) is
> the source of truth for scope, score/search contracts, tests, and phase gates.
> This document is retained for rationale. Sections marked **Superseded** below
> conflict with the plan; follow the linked plan contract instead. In particular,
> the plan supersedes the old terminal/depth order, forced-pass root results,
> TT `solved` rules, static X/C penalties, Wilson statistics, seeded openings,
> production D4/harness/baselines, the 1000-line metric, and the open questions
> listed at the end.

## Goals and non-goals

The goal is a classical Othello engine (bitboards, alpha-beta search, hand-built evaluation) in under 1000 lines of `std`-only Rust, written to teach the fundamentals before any machine learning is added. The 1000-line limit is a ceiling. Where shorter conflicts with more obvious, take more obvious. **The plan operationalizes this as `<=999` formatted non-comment lines for the engine plus playable CLI, with experiment tooling in a separate 600-line `tools/` crate; see [Constraints and targets](PLAN.md#constraints-and-targets).**

**Goals**

- A correct move generator, verified two independent ways (a naive reference and perft).
- A search that plays a perfect endgame from about 12 empties and sensible midgames above that.
- A tools match harness that gives win rates with confidence intervals, so every heuristic and search change is tested rather than assumed.
- A structure that lets a learned evaluator replace the hand-built one later with a one-function change.

**Non-goals**

- Beating Edax. A strong engine needs pattern tables fitted on millions of positions, a tuned opening book and heavy search optimisation. None of that teaches the fundamentals efficiently.
- A GUI or a UCI-style protocol. A board printer and a simple CLI are enough.
- Dependencies. A PRNG is 8 lines of xorshift. Pulling in `rand` hides nothing worth learning.

## Bitboard core

The board is two `u64`s stored relative to the side to move, and a move is found by shifting whole bitboards one direction at a time. Everything else in the engine sits on this layer, so it gets the most care.

**Representation**

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
struct Board { me: u64, opp: u64 }   // bit i = square i, a1 = bit 0, h8 = bit 63
```

Storing "me" and "opp" instead of "black" and "white" removes the player-flipping bugs that plague value targets and negamax signs. `play(sq)` returns a new `Board` with the two fields already swapped, so the child is automatically from the opponent's point of view. A pass is `Board { me: opp, opp: me }`.

**Direction shifts**

A bitboard is a 1D integer, so a left shift by 1 moves h-file discs onto the a-file of the next rank. Each direction needs a mask that discards wrapped bits.

| Direction | Shift | Mask applied after |
| --- | --- | --- |
| East | << 1 | not file A |
| West | >> 1 | not file H |
| North | << 8 | none (falls off the top) |
| South | >> 8 | none |
| North-east | << 9 | not file A |
| North-west | << 7 | not file H |
| South-east | >> 7 | not file A |
| South-west | >> 9 | not file H |

**Legal moves**

For each direction, start from your discs, shift once and keep opponent discs, then keep shifting and accumulating through opponent discs (at most 5 more steps, since a run of 6 opponent discs is the longest that can sit between your disc and an empty square). One more shift landing on an empty square is a legal move. The 8 directions are independent and OR together into the move set.

**Flips**

For a chosen square, walk each direction while you see opponent discs. Keep that run only if it ends on one of your own discs. Written as a plain loop over 8 direction offsets with per-step edge checks, this is 15 to 25 lines. Resist the branchless version until the plain one is tested. A flip routine you cannot verify by reading is worse than a slower one you can.

**Terminal and pass handling**

- If the side to move has no legal moves but the opponent does, pass.
- If neither has a move, the game is over. **Superseded by [Score and search contracts](PLAN.md#score-and-search-contracts):** score is the WTHOR disc difference in centidiscs; the winner receives empty squares; a tied terminal with empty squares scores zero.
- In search, a pass costs no disc and flips the side to move. **Superseded by [Terminal, pass, and the root](PLAN.md#terminal-pass-and-the-root):** terminal and pass detection precede the depth cutoff, and a pass costs no depth. `search` requires a legal root move; callers handle pass and terminal roots. Whether a pass counts as a ply in perft remains a convention that must match the table you compare against (see testing).

**Maths to understand**

- Bitwise operations as set operations: `&` is intersection, `|` is union, `!` is complement.
- Why masks are needed: the board is `row * 8 + col`, and shifts do not know about row boundaries.
- Popcount (`count_ones`) gives disc counts and mobility in one instruction.

## Correctness testing

The bitboard code is verified against a deliberately naive implementation and against perft counts, and nothing else is written until both agree. Search and evaluation bugs are hard enough to find without a faulty move generator underneath them.

**1. Naive reference (about 50 lines, `#[cfg(test)]`)**

An `[[i8; 8]; 8]` array with nested loops: for each empty square and each of 8 directions, walk while opponent discs are seen, and check the run ends on your disc. It is slow and obviously correct by inspection, which is the point. It also doubles as a readable statement of the rules.

Test: generate several thousand random positions by playing random legal moves from the start, and at every position compare the reference and the bitboard version on (a) the legal move set and (b) the resulting board after each legal move. Any mismatch prints the position and the move.

**2. Perft**

Perft(d) counts the leaf nodes of the full game tree to depth d. From the start position the commonly published values are:

| Depth | Nodes |
| --- | --- |
| 1 | 4 |
| 2 | 12 |
| 3 | 56 |
| 4 | 244 |
| 5 | 1,396 |
| 6 | 8,200 |
| 7 | 55,092 |
| 8 | 390,216 |

These are from memory, so cross-check them against a published source before trusting a mismatch in either direction. Passes cannot occur this early, so up to about depth 8 the values do not depend on pass handling. Deeper tables differ by convention (whether a pass consumes a ply, and how finished games are counted). Fix a convention, write it in the test, and only compare against tables that use the same one.

> **Superseded by [Test strategy](PLAN.md#test-strategy):** verified depths 1-10
> are `4, 12, 56, 244, 1396, 8200, 55092, 390216, 3,005,288, 24,571,284` under
> the plan's pass convention. Add hand-built pass and early-terminal fixtures;
> the initial-position table through depth 10 does not exercise early
> termination.

**3. Symmetry test**

Othello's start position and rules have the 8-fold symmetry of the square (the dihedral group D4). Implement the transforms for tests and canonical position keys, and test that `legal_moves(transform(b)) == transform(legal_moves(b))` on random positions. This catches mask errors that perft can miss. **Superseded by [File layout](PLAN.md#file-layout):** D4 is not production engine code; it lives in tests/tools unless runtime augmentation is funded from the engine budget.

**4. Invariants checked inside the tests**

- `me & opp == 0` always.
- Disc count after a move equals the previous count plus one.
- Playing a move flips at least one disc.
- Passing twice in a row is terminal.

**What this does not cover**

Perft with compensating errors can pass, which is rare but possible. That is why the naive comparison comes first. Neither check validates the evaluation or search. Those are tested by playing games (see the match harness).

## Evaluation heuristics

The evaluator is a linear combination of a few features, with weights that change by game phase. Build it up one feature at a time and keep a feature only if the match harness shows it helps.

| Feature | What it measures | Cost | When it matters |
| --- | --- | --- | --- |
| Positional weight table | Static value per square | Low | Opening and midgame; weak in isolation |
| Mobility | Own moves minus opponent moves | One `legal_moves` call per side plus popcount | Midgame, the strongest dynamic signal |
| Frontier discs | Own discs adjacent to empty squares | 8 shifts plus popcount | Opening and midgame |
| Corners | Corner count difference | Two masks and popcount | Always |
| X/C-square risk | Occupying squares next to an empty corner | Mask per corner | Opening and midgame |
| Stability | Discs that can no longer be flipped | Moderate to high | Late midgame onward |
| Disc difference | Raw disc count | Popcount | Heuristic region above the 12-empty solver threshold |

**Positional weight table**

A fixed score per square, summed over own discs minus opponent discs. A common starting shape (the exact numbers vary between sources and are only a starting point):

```
100 -20  10   5   5  10 -20 100
-20 -50  -2  -2  -2  -2 -50 -20
 10  -2  -1  -1  -1  -1  -2  10
  5  -2  -1  -1  -1  -1  -2   5
  5  -2  -1  -1  -1  -1  -2   5
 10  -2  -1  -1  -1  -1  -2  10
-20 -50  -2  -2  -2  -2 -50 -20
100 -20  10   5   5  10 -20 100
```

Corners are valuable because they can never be flipped. X-squares (diagonal to a corner) and C-squares (edge-adjacent to a corner) are bad while the corner is empty, because they tend to hand the corner to the opponent. The table cannot express that condition, so an X-square next to an already-taken corner is wrongly penalised. That is the main weakness, and the reason for the explicit corner-risk term.

> **Superseded by [Feature contract](PLAN.md#feature-contract):** do not use
> permanent static X/C penalties. Replace the table's static X/C values with a
> neutral base value and apply the X/C penalty only while the adjacent corner is
> empty.

**Mobility and frontier**

Restricting the opponent's options forces them into bad moves. A normalised form keeps the term bounded:

```latex
M = \frac{m_{me} - m_{opp}}{m_{me} + m_{opp} + 1}
```

> **Superseded by [Feature contract](PLAN.md#feature-contract):** scale before
> integer division: `(m_me - m_opp) * 1000 / (m_me + m_opp + 1)`, so unequal
> counts never round to zero. Frontier is `opp_frontier - own_frontier` with a
> positive weight.

Frontier discs give the opponent future moves, so fewer is better. With `empty_nbrs` as the union of all 8 shifts of the empty set, the frontier count is `popcount(empty_nbrs & me)`.

**Stability**

Exact stability is awkward. Use a conservative approximation and accept undercounting, as long as it is consistent: discs connected to an owned corner along a full or contiguous edge run, plus discs whose row, column and both diagonals are all full. Add this last, after the cheaper terms are tuned, because it is the most code for the least certain gain.

**Disc difference and parity**

Early on, having fewer discs is often better, because it keeps your own mobility high. Give disc difference a weight near zero until the endgame, then let it take over. Parity (moving last in each empty region) is handled automatically once the endgame is solved exactly.

> **Superseded by [Feature contract](PLAN.md#feature-contract):** with an exact
> threshold of 12 empties, a disc-diff feature active only below roughly 10
> empties is normally bypassed by the solver. Make disc diff active in the
> heuristic region above the 12-empty threshold and tune it where the heuristic
> actually runs.

**Combining by game phase**

The classical approach (Rosenbloom's IAGO, Lee and Mahajan's BILL) uses coefficients that depend on move number or empty count:

```latex
E(s) = \sum_i w_i(t)\, f_i(s)
```

Start with a linear interpolation of each weight between an opening value and an endgame value, indexed by disc count. Three discrete phases (early, middle, late) are a fine first step and easier to tune by hand.

**Honest expectations**

Hand-tuned weights are guesses. Many features will do nothing, and some will hurt. The only way to know is to run the tools harness with alternating colours and frozen openings. The strong engines (Logistello, Edax) do not hand-tune any of this. They fit lookup tables over fixed square patterns (edges plus X-squares, corner 3x3 blocks, rows, diagonals) by regression on large numbers of positions, one table set per game stage. That is the natural bridge to the learning part of this project.

## Search

The search is negamax with alpha-beta pruning, driven by iterative deepening, with an exact solver for the endgame. Move ordering decides how much pruning you actually get, so it is worth more effort than the transposition table.

**Negamax with alpha-beta**

> **Superseded by [Terminal, pass, and the root](PLAN.md#terminal-pass-and-the-root)
> and [Iterative deepening and cancellation](PLAN.md#iterative-deepening-and-cancellation).**
> The historical pseudocode below checks `depth == 0` before terminal/pass and
> hardcodes `eval(b)`; do not copy it. The plan requires terminal/pass before
> the cutoff, a search configuration carrying the evaluator, true fail-soft,
> `Option<i32>` cancellation, `SearchStatus { Complete, Partial, Fallback }`,
> and `best: u8` with a legal root move. Callers handle pass and terminal
> roots; `search` is never called on a forced-pass root.

Othello is zero-sum, so a position's value for one side is the negative of its value for the other. One function covers both players:

```rust
fn negamax(b: Board, depth: u32, mut alpha: i32, beta: i32) -> i32 {
    if depth == 0 { return eval(b); }
    let moves = legal_moves(b.me, b.opp);
    if moves == 0 {
        if legal_moves(b.opp, b.me) == 0 { return final_score(b); }
        return -negamax(b.pass(), depth, -beta, -alpha);   // pass costs no depth
    }
    for sq in ordered(b, moves) {
        let v = -negamax(b.play(sq), depth - 1, -beta, -alpha);
        if v >= beta { return v; }      // cutoff
        if v > alpha { alpha = v; }
    }
    alpha
}
```

The negation and the swapped window `(-beta, -alpha)` are the whole trick. Forgetting either is the most common bug. With perfect move ordering, alpha-beta searches about the square root of the nodes plain minimax needs, which roughly doubles the reachable depth.

**Iterative deepening**

Search depth 1, then 2, then 3, until a time or depth limit. It looks wasteful, but the tree grows geometrically so the repeated shallow searches cost little, and each iteration supplies the best move to try first in the next one. That ordering is where the pruning comes from. It also gives a usable answer whenever time runs out.

**Move ordering**

In order of priority:

1. The best move from the previous iteration (or the transposition table).
2. Corners.
3. Moves that leave the opponent the fewest replies (a cheap mobility sort).
4. X-squares and C-squares last.

Measure it. Count nodes searched at a fixed depth with and without each ordering rule.

**Transposition table**

Different move orders reach the same position. A hash table keyed on the position stores the value, depth, move and bound type (exact, lower or upper bound) so repeated positions are not searched again.

> **Superseded by [Transposition table contract](PLAN.md#transposition-table-contract):**
> the plan uses separate `Domain { Heuristic, Endgame }` and
> `Bound { Exact, Lower, Upper }`, strict equal-depth heuristic reuse, reusable
> endgame bounds, and no incomplete stores. Do not encode domain through a
> `solved` flag. The plan uses const-fn Zobrist tables, stores the full 64-bit
> key, and treats `key == 0` as the empty sentinel. A full key detects index
> collisions but not full-key aliases; accept that residual risk. A table
> belongs to one evaluator/search configuration and is cleared between tools
> harness games, not aged.

- Use const-fn splitmix64 Zobrist tables and XOR the keys of occupied squares. Index by `key % len` or a multiply mix, store the full key, and compare it on probe. Accept full-key alias risk.
- The bound type is the part people get wrong. A value returned after a cutoff is only a bound, not the exact score.

Add it after plain alpha-beta with ordering works and is tested. It helps mainly in the midgame and is a source of subtle bugs.

**Endgame solver**

With about 12 empties or fewer, search to the end of the game with disc difference as the score and no heuristic. A bitboard engine can do this in seconds at 12 to 14 empties. It reuses the same negamax with a different evaluation and unbounded depth. Use a narrow window (-1, +1) first to answer win, draw or loss cheaply, then widen it if you want the exact margin.

> **Superseded by [Transposition table contract](PLAN.md#transposition-table-contract)
> and [Phase 6](PLAN.md#phase-6-exact-endgame-solver-and-completion-metrics):**
> use `terminal_score` in centidiscs. The solver is triggered at the root only,
> runs WDL then exact, and never runs at interior nodes. Endgame entries are a
> separate domain with exact values and reusable bounds; a draw proven inside
> `(-1, 1)` may be stored as exact zero. Heuristic search never consumes endgame
> entries, and the solver never consumes heuristic entries.

**What to measure**

Nodes per second, nodes at fixed depth with different orderings, and the depth reached in a fixed time budget. Strength is measured separately, by matches.

## Match harness

Every change to evaluation or search is judged by playing games, so the harness is built before any tuning starts. A feature that looks sensible and loses matches is common in Othello.

**Design**

> **Superseded by [Phase 3](PLAN.md#phase-3-tools-harness-frozen-evaluator-position-based-splits):**
> the harness lives in a separate tools crate. Agents are created per game from
> factories, the absolute side-to-move is explicit outside the side-relative
> board, release matches validate actions, position sets use canonical D4 keys
> with tune/validation/final splits, and paired results use a pair bootstrap with
> a degeneracy policy.

- An agent is any policy/factory, so the harness works for the alpha-beta engine, a random player, a greedy player and a later neural-network player alike. The plan requires a per-game factory or reset lifecycle.
- Games start from one frozen opening source in `bench/`, not seeded generation. Openings are grouped by the canonical D4 position after ply 4, and all openings in a group go to one split.
- Each opening is played twice with colours swapped, so opening luck cancels.
- Agent and bootstrap RNG streams are derived from stable run/pair/game IDs and logged. There is no opening RNG stream.

**Reporting**

Win rate with a confidence interval, draws counted as half a win. The Wilson interval is better behaved than the normal approximation at small sample sizes and at extreme win rates:

```latex
\hat p \pm z\,\sqrt{\frac{\hat p(1-\hat p)}{n}}
```

That is the simple normal form. The Wilson form adds a correction to the centre and width, and is about 10 lines of code. Use it.

> **Superseded by [Statistics policy](PLAN.md#statistics-policy):** paired
> results use a pair bootstrap, not Wilson. Keep Wilson only for unpaired
> binomial diagnostics. Small or degenerate samples use a conservative
> bounded-mean fallback and withhold a strength claim; Elo endpoints come from
> finite interval endpoints.

To convert a win rate into an Elo difference, invert the expected-score formula:

```latex
E = \frac{1}{1 + 10^{(R_B - R_A)/400}}, \qquad R_A - R_B = 400\,\log_{10}\frac{E}{1-E}
```

**Sample size**

A 55% win rate over 100 pairs is within noise. Distinguishing a 5-point strength gap generally takes several hundred pairs, and smaller gaps take thousands. A 55% equal-time claim uses about 400 pairs. Parallel tools matches are optional, need CPU-contention controls, and serial remains the reference.

**Baseline ladder**

1. Random.
2. Greedy (maximise discs after the move).
3. Alpha-beta with only the positional weight table, at a fixed depth.
4. Each later version against the previous one.
5. Edax at a low level, as an external yardstick (outside the 999-line engine, driven through its text protocol).

Writing the alpha-beta player yourself is the point of this phase. It is also the yardstick the neural-network work is later compared against.

## Line budget

The estimated total is 460 to 710 lines, which leaves headroom under the ceiling. These are estimates, not measurements, and tests are counted separately in practice.

> **Superseded by [Constraints and targets](PLAN.md#constraints-and-targets):**
> the hard ceiling is `<=999` formatted non-comment lines for the engine plus
> playable CLI, not 1000 lines under `src/`. Experiment tooling lives in a
> separate 600-line `tools/` crate. Tests are excluded, but implementation may
> not hide in tests, `include!`, `#[path]`, or `build.rs` sources. Phase 2 must
> prove a complete vertical slice with actual counts.

| Component | Lines |
| --- | --- |
| Bitboard core (shifts, legal moves, flips, `play`, pass, terminal) | 80-130 |
| Perft and tests | 40-60 |
| Evaluation (weight table, mobility, frontier, corners, phase weights) | 80-120 |
| Negamax, alpha-beta, iterative deepening, move ordering | 100-210 |
| Transposition table and Zobrist | 50-80 |
| Endgame solver (reuses negamax, root only) | 20-40 |
| CLI and board printer | 80-160 |
| Tools harness, baselines, stats, positions (separate budget) | 400-600 |

**Where lines stay cheap:** `Board::play` returning an already-swapped board removes most bookkeeping, and negamax is short.

**Where they creep up:** a proper stability calculation, an opening book, a protocol layer, and test code. Skip these at first.

**Rules for staying under budget without hurting readability**

- One crate, a few modules, `std` only.
- The evaluator is a plain function of `Board`, not a trait hierarchy. Refactor into something swappable when the learned evaluator arrives.
- Each function does one thing that can be stated in one sentence, and can be tested alone.
- When a choice is between shorter and more obvious, choose obvious. Density and length are different things, and a bit trick that saves 20 lines but takes an hour to verify is a bad trade at this stage.

## Later extensions

Once the classical engine is tested and measured, the learning work builds on it. The classical engine supplies the move generator, the baseline opponent and the evaluation harness.

**1. Learned pattern evaluator**

Replace the hand-tuned evaluation with lookup tables indexed by the configuration of fixed square sets (edges, corner blocks, rows, diagonals), with one table set per game stage. Each square is empty, own or opponent, so a pattern of n squares has 3^n entries. Fit the weights by linear regression, or by temporal-difference learning in the n-tuple style. It plugs in as a different `eval` function and the match harness measures the gain directly. This is the closest classical analogue of what Logistello and Edax do, and a good intermediate step before neural networks.

**2. Supervised pretraining**

Replay WTHOR tournament games through the engine and emit, per position, the input planes, the move played (policy target) and the final disc differential scaled to \[-1, 1\] (value target). Split train and validation by game, not by position. Positions within one game are correlated and splitting by position leaks. Verify the file format against the WTHOR specification rather than assuming it. The 8-fold symmetry from the testing section gives data augmentation.

**3. Policy and value network**

A small residual network with two heads, trained in PyTorch with the Rust engine exposed through PyO3. The loss is policy cross-entropy plus a weighted value MSE, with illegal moves masked before the softmax.

**4. MCTS with PUCT, then self-play**

Wrap the network in Monte Carlo tree search using the PUCT selection rule, with value negation on backup. Then train on visit-count distributions and game outcomes from self-play. Develop on 6x6 first, where learning is visible in minutes.

Each stage is judged with the same harness against the alpha-beta engine from this document. Expect a policy trained only on human games to beat random and greedy easily and to lose to a decent alpha-beta engine. Better than alpha-beta only appears, if at all, after search and self-play are working.

## Open questions, risks and reading

**Open questions**

> **Resolved by [Decisions resolved from the design's open questions](PLAN.md#decisions-resolved-from-the-designs-open-questions):**
> WTHOR score with empty squares to the winner; 1000 ms default per move;
> tuning uses fixed node budgets with short controls, and a 55% equal-time
> claim needs about 400 pairs. Edax is out of scope for the 999-line engine and
> may remain an optional external script.

**Risks**

- Passing perft while the evaluator or search is wrong. Perft only validates move generation.
- Over-tuning on a small number of tools harness games. About 400 pairs is the minimum for a 55% equal-time claim; smaller misses are inconclusive.
- Transposition table bugs, mostly bound-type handling and Zobrist collisions. Store the full key and compare it on probe; accept full-key alias risk. Test search results with and without the table at a fixed depth and equal remaining depth, including a deep-then-shallow sequence; they must match.
- Figures in this document that came from memory, notably the perft values and the line estimates, are unverified.

**Reading**

- Chess Programming Wiki pages on bitboards, perft, negamax, alpha-beta and transposition tables. The techniques transfer directly.
- Rosenbloom, IAGO, and Lee and Mahajan, BILL, for the classical evaluation features.
- Buro's papers on Logistello, for pattern-based evaluation.
- Lucas and colleagues on n-tuple networks for Othello.
- Silver et al., AlphaGo Zero and AlphaZero, for the later MCTS and self-play stage.
- Sutton and Barto, chapters on temporal-difference learning, for the context behind fitted evaluators.
