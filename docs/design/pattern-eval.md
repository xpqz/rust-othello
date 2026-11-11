# Learned Pattern Evaluation: Design

Nov 11, 2026 · Draft, revision 2

> Scope: replace the hand-written evaluator with pattern tables learned from
> human tournament games, and add the trainer that produces them. The search
> is not changed. `docs/PLAN.md` remains the source of truth for the
> statistics policy and process; this document changes three budgets, listed
> under [Budgets](#budgets). Revision 2 answers an external review;
> [Review response](#review-response) maps each finding to its change.

## Why

This is the main reason the project exists. Two aims, in this order:

1. **Understanding.** A reader should be able to open the code and see how
   an engine learns to judge positions: where the examples come from, what
   is adjusted, and how we know it worked.
2. **Strength.** A clear win over release 1.0.0 at 3 s per move.

Size discipline serves the first aim. A learning system spread over tens of
thousands of lines teaches nobody. The target is a player and the program
that taught it in under 2,000 lines.

## The idea

### What an evaluator does

When the search runs out of depth it must guess the final disc difference of
a position without looking further. Today that guess is `src/eval.rs`: a
table of square weights plus mobility, frontier and corner-risk terms, about
a dozen hand-chosen numbers. It knows that corners are good. It cannot know
that one particular arrangement of an edge is a trap.

### Patterns

A **pattern** is a fixed group of squares. Each square is empty, mine or
theirs, so a pattern of *n* squares has 3ⁿ possible **configurations**.
Number them from 0 to 3ⁿ − 1 by reading the squares as base-3 digits
(0 empty, 1 mine, 2 theirs). Keep a table with one value per configuration:
"this exact arrangement is worth +3.2 discs to the player to move".

The evaluation is the sum of the table values for every pattern on the
board. It is fast because it is only lookups and additions.

It works because Othello is decided by local shapes: edges, corners and the
lines running into them. A table stores a separate value for every
arrangement of its squares, so it captures interactions that no short list
of rules can. Across patterns the model is a plain sum; within a pattern it
is unrestricted.

### The eleven families

Boards are drawn with a1 at the top left; `X` marks the squares of the base
pattern. Each family is listed once and placed on the board in every
distinct rotation and reflection, which gives its **instances**.

```
edge + 2X  (10 squares, 4 instances)   corner 3x3  (9 squares, 4 instances)
  a b c d e f g h                        a b c d e f g h
1 X X X X X X X X                      1 X X X . . . . .
2 . X . . . . X .                      2 X X X . . . . .
3 . . . . . . . .                      3 X X X . . . . .

corner 2x5  (10 squares, 8 instances)  rows 2, 3, 4  (8 squares, 4 each)
  a b c d e f g h                        a b c d e f g h
1 X X X X X . . .                      2 X X X X X X X X   (row 2 shown)
2 X X X X X . . .

diagonals of length 8, 7, 6, 5, 4  (2, 4, 4, 4, 4 instances)
  a b c d e f g h
1 X . . . . . . .      length 8 runs a1-h8
2 . X . . . . . .      length 7 runs b1-h7
3 . . X . . . . .      length 6 runs c1-h6
4 . . . X . . . .      length 5 runs d1-h5
5 . . . . X . . .      length 4 runs e1-h4
```

| Family | Squares | Configurations | Instances |
| --- | ---: | ---: | ---: |
| Edge + 2X | 10 | 59,049 | 4 |
| Corner 3x3 | 9 | 19,683 | 4 |
| Corner 2x5 | 10 | 59,049 | 8 |
| Row 2 | 8 | 6,561 | 4 |
| Row 3 | 8 | 6,561 | 4 |
| Row 4 | 8 | 6,561 | 4 |
| Diagonal 8 | 8 | 6,561 | 2 |
| Diagonal 7 | 7 | 2,187 | 4 |
| Diagonal 6 | 6 | 729 | 4 |
| Diagonal 5 | 5 | 243 | 4 |
| Diagonal 4 | 4 | 81 | 4 |
| **Total** | | **167,265** | **46** |

This is the family set used, with small variations, by Logistello, Zebra and
Edax. Every instance of a family shares that family's table, so the four
edges teach one edge table.

### Stages

A shape's worth changes through the game: an edge that is a liability at
move 20 can be an asset at move 50. So there is one complete set of tables
per **stage**. Stage is `(60 - empties) / 5`, twelve stages of five plies
each. The stages are independent; neighbouring stages are not blended in
this version.

### One point of view

The engine's `Board` is side-relative: `me` is the player to move. The
tables are learned from the same point of view. Digit 1 is always the player
to move, and every value is a disc difference for the player to move. One
set of tables serves both colours, and the value of having the move is
learned along with everything else.

This changes an existing contract. `docs/PLAN.md` requires the evaluator to
be antisymmetric, `evaluate(b) == -evaluate(pass(b))`, and for that reason
rules out a tempo term. The learned evaluator is deliberately not
antisymmetric: passing changes whose move it is, which is a different
position with a different value. Negamax does not need antisymmetry. The
plan's sentence is rewritten, the antisymmetry test stays with the classic
evaluator, and it is not applied to the learned one.

### How the tables are learned

The model is a sum, so fitting it is linear regression. The procedure is a
loop of guess, measure the miss, and nudge.

- **Examples.** Positions with a known answer: the final disc difference for
  the player to move.
- **Start.** Every table value is zero.
- **One pass over the examples:**
  1. *Guess.* For each position, sum the 46 values its patterns select.
  2. *Measure.* The residual is answer minus guess.
  3. *Nudge.* Each table value moves by a fraction of the average residual
     of the positions in which it took part.
- **Repeat** until the error on games held back from training stops
  improving.

Why this finds sensible values: a given edge shape appears in thousands of
positions. Where the guess was too low it is pushed up, where too high,
down, and it settles where these balance. Each value is only charged for the
part of the miss that the other 45 patterns on the board did not explain, so
a good edge that tends to come with a bad corner still receives its own
value. That separation is what regression does and a simple average does
not.

Because the model is a sum, the error is a bowl with no false bottoms: the
loop cannot get trapped at a poor fit, which is not true of neural networks.
The bottom of the bowl is flat in some directions, though. Every position
has exactly four edges and four 3x3 corners, so adding one disc to every
edge value and taking one from every corner value changes no evaluation.
The fit therefore pins down the *sum* for a position, not each table value
on its own. Starting from zero and nudging gives one consistent answer, but
a single table value means little in isolation; compare values within one
family at one stage.

Three things go wrong, each with a plain remedy:

- **Rare shapes.** Many of the 59,049 edge configurations appear a handful
  of times. Their average residual is noise. The step for a value is divided
  by its number of appearances, with a floor, so a shape seen three times
  moves as slowly as one seen many times. This is damping, not a permanent
  limit: given enough passes a rare value would still fit its few noisy
  examples. It stays near zero because training stops early, when held-out
  error stops improving, before the slow values have travelled far. Buro
  describes this mechanism for Logistello and attributes its effect to the
  finite number of passes. A shape never seen stays exactly zero: no
  opinion.
- **Memorising.** With two million values the tables can learn the training
  games by heart. Whole games are held back, never trained on, and training
  stops when the error on them stops improving.
- **Wrong answers.** The tables are only as good as the labels. See
  [Labels](#labels).

## Decisions

Decided by the user:

- Pattern tables, not a neural network.
- Training data is the public WTHOR archive of human tournament games.
- The trainer is held to the same standard as the engine: counted, guarded
  by a budget test, same rules on comments, naming and honest code.
- Success is a clear win over 1.0.0 at 3 s per move.
- The budgets under [Budgets](#budgets). Any overrun is discussed with the
  user before it happens.
- The engine stays dependency-free; the trainer may use any crate. See
  [Dependencies](#dependencies).

Design calls made here, open to challenge:

- The eleven families and twelve stages above.
- Pattern indexes are computed from the board at each evaluation. There is
  no incremental update as moves are made.
- No terms besides the tables: no mobility, parity or constant term.
- Labels are repaired as described under [Labels](#labels).
- The trainer is its own crate.
- The opponent for the gate is the 1.0.0 evaluator running in process.

## Budgets

| Part | Now | Budget | Guard |
| --- | ---: | ---: | --- |
| Engine + CLI | 998 | 1,499 (unchanged) | `tests/budget.rs` |
| of which evaluator, after the switch | 81 | 230 | reported in notes |
| Engine total after this epic | | 1,150 | reported in notes |
| Trainer crate (new) | 0 | 450 | `trainer/tests/budget.rs` |
| Tools crate | 1,169 | 1,300 (was 1,200) | `tools/tests/budget.rs` |
| Weights asset | none | 4 MiB (plan allowed 8) | engine test |

- **Correction to the figures discussed.** The evaluator budget was first
  stated as 250 with the engine at 1,150. Those do not add up: 998 − 81 +
  250 is 1,167. The evaluator budget is 230.
- While the old and new evaluators coexist, before the switch, the engine
  may reach 1,230.
- **Two figures were raised in revision 2 and approved by the user.**
  - *Tools 1,300*, from 1,200. The 1.0.0 evaluator moves into
    `tools/src/baselines.rs` as a frozen opponent (about 80 lines), which
    did not fit in the 31 lines left.
  - *Trainer 450*, from the 350 first discussed. Revision 2 added the
    restart protocol, exact weight sharing, the layout record and the
    quantisation checks. The estimate: reader and replay 75, labelling 60,
    examples and labels 45, weight sharing 30, the fitting loop 70, restart
    50, report 40, asset and layout 30, `inspect` 35, command line 25.
- **Going beyond any figure in this table is a conversation with the user
  first**, not a decision for the builder or the reviewer.
- The trainer counts under the same metric and the same anti-evasion rules
  as the engine: no `include!`, no `#[path]`, no generated source, no test
  code in `src`.
- If the evaluator cannot fit in 230 lines without contortion, drop a
  family (corner 2x5 first) and say so. Do not squeeze the code.

## Dependencies

Decided by the user: the engine stays lean and dependency-free; the trainer
may use any crate it wants.

- **Engine.** `std` only, no dependencies, as now. It is the part a reader
  studies first, and it needs nothing.
- **Trainer.** No restriction on dependencies. Use a crate wherever it
  replaces plumbing a reader gains nothing from seeing written out. Each
  dependency gets a one-line reason in the trainer's notes.
- **Lined up:**
  - `sha2` 0.11 (RustCrypto, pure Rust): SHA-256 for the WTHOR manifest,
    the label journal and the asset. Hand-written, it would take about 60
    of the trainer's 450 lines.
  - `rayon`: parallel labelling by game and parallel fitting by stage,
    in place of hand-rolled thread and channel code.
  - Others at the builder's discretion.
- **One limit, which is a design call and not a dependency rule.** The
  regression loop itself stays our own code: the guess, the residual and
  the nudge. Seeing it is the first aim of this work, and it is about sixty
  lines. A fitting or machine-learning crate would hide exactly the part
  the reader came for.
- **Tools crate.** Unchanged: it depends on the engine only. Nothing here
  needs more, and the user's ruling named the trainer. This is a design
  call.
- **The guard.** The engine's budget test asserts that its `Cargo.toml` has
  no dependencies. The trainer has no such test.
- `Cargo.lock` is committed, so builds are pinned.

## Architecture

```
WTHOR files ──> trainer: read, replay ──> positions
                trainer: label (long job, journaled) ──> answers
                trainer: fit ──> assets/eval.bin + learning report
                                      │
engine: src/pattern.rs  <─────────────┘
        features(board) ──> 46 table indexes   (also called by the trainer)
        evaluate(board) ──> sum of 46 values
```

The pattern definitions and the function that turns a board into table
indexes live in the engine and nowhere else. The trainer calls that
function. The engine and the trainer therefore cannot disagree about what a
table index means.

### Engine: `src/pattern.rs`

- **Families.** Eleven base square lists, as constants. The order of
  squares in a list is the order of base-3 digits, first square least
  significant.
- **Instances.** Built once at first use by applying the eight board
  symmetries to each base list. Two transformed lists are the same instance
  when they cover the same *set* of squares; the first one generated is
  kept, with its squares in transformed base order. The result is 46
  instances, each an ordered list of squares plus the offset of its
  family's table. `std::sync::LazyLock` is acceptable.
- **`features(board) -> [u32; 46]`.** For each instance, the family offset
  plus the base-3 index of its squares (0 empty, 1 `me`, 2 `opp`). Public,
  used by the trainer. Entries may repeat: at the start position all four
  edges select the same all-empty entry.
- **`stage(empties) -> usize`.** `60.saturating_sub(empties) / 5`, at most
  11. Test boards with more than 60 empties must not underflow.
- **`evaluate(board) -> i32`.** Sums the 46 values for the board's stage and
  clamps to `±EVAL_BOUND`. It has the signature of `SearchConfig.eval`, so
  the search needs no change.
- **Weights.** `include_bytes!("../assets/eval.bin")`, read two bytes at a
  time with `i16::from_le_bytes`. No allocation, no start-up step, no
  `unsafe`.

How the base-3 index is computed (square by square, or with a byte lookup
for whole rows) is left to the builder. Correctness is fixed by the tests;
speed is measured, see [Speed and diagnostics](#speed-and-diagnostics).

### Shared weights within a family

Some families map onto themselves under a board symmetry. The two readings
of such a pattern are the same shape, so their table entries must hold the
same value. The permutations are exact and differ by family:

| Family | Symmetry that preserves its squares | Effect on the square list |
| --- | --- | --- |
| Edge + 2X | Left-right mirror | Edge reversed; b2 and g2 swapped |
| Corner 3x3 | Reflection in the a1-h8 diagonal | Transposed |
| Corner 2x5 | None | No sharing |
| Rows 2, 3, 4 | Left-right mirror | Reversed |
| Diagonals 8 to 4 | Reflection in the crossing diagonal | Reversed |

The corner 2x5 block has eight distinct placements and no symmetry of its
own. Its entries are not tied; reversing it would equate squares that play
differently.

The trainer derives each permutation from the base list and the symmetry,
maps every index to the smaller of itself and its image, and learns one
value per such pair. Writing the asset copies the value to both entries.
Sharing is therefore exact by construction, not a property that training
happens to approach.

### Asset: `assets/eval.bin`

- 12 stages × 167,265 values × 2 bytes = 4,014,360 bytes exactly.
- Values are little-endian `i16` in centidiscs, the engine's score unit.
- Order: stage 0 first; within a stage, families in the table's order;
  within a family, by configuration index.
- The include guard in `tests/budget/rules.rs` is relaxed to allow
  `include_bytes!` of a path under `assets/` only, with a test. An engine
  test asserts the file's exact size and that the total under `assets/` is
  at most 4 MiB.
- The file is committed. It is reproducible from the trainer at a recorded
  commit, the WTHOR files' SHA-256 manifest and the label journal's SHA-256.
- `assets/` does not exist yet. The trainer depends only on
  `pattern::features`, so the first fit runs before `pattern::evaluate`
  lands; no placeholder asset is needed.
- **Layout identity.** Sharing `features` stops the engine and trainer
  disagreeing within one build. It does not stop an old asset being read
  under changed pattern definitions. `assets/eval.layout`, a committed text
  file written by the fit, records: a layout fingerprint (a hash over every
  instance's ordered squares, the family offsets, the digit convention and
  the stage boundaries), the asset's SHA-256, the trainer commit, and the
  expected `evaluate` result for a few fixed boards. Engine tests recompute
  the fingerprint from `pattern.rs` and evaluate those boards through the
  real loader; both must match the file.
- **Quantisation.** A value that is not finite, or that falls outside
  `i16` after conversion to centidiscs, fails the fit with an error naming
  the entry. Values are never silently saturated. The fit log records the
  largest absolute value. A trainer test compares the trainer's own
  prediction with the engine's `evaluate` on held-out positions after
  rounding and clamping: they may differ by at most 0.23 discs (46 values,
  half a centidisc each) wherever the clamp is not reached.

### Trainer: `trainer/`

A new crate, `othello-trainer`, depending on the engine (board, solver,
`pattern::features`), on the tools crate (journal) and on external crates
as [Dependencies](#dependencies) allows. It has no `unsafe` of its own. The
engine-commit lookup and the file hash that journal headers need are private
to `tools/src/main.rs` today; they move into the tools library so the
trainer can share them. Four commands:

| Command | Does | Runtime |
| --- | --- | --- |
| `check` | Verifies the WTHOR files and replays every game | seconds |
| `label` | Solves late positions exactly; journaled and resumable | hours |
| `fit` | Learns the tables; writes the asset and the report; restartable | minutes |
| `inspect` | Prints the best and worst shapes of a family at a stage | instant |

`scripts/check.sh` builds, tests and lints the trainer like the other
crates.

## Data

### The archive

- Source: the WTHOR database of the Fédération Française d'Othello,
  `https://www.ffothello.org/wthor/base/WTH_YYYY.wtb`, one file per year,
  1977 to 2025. The federation's page gives 137,548 games (files last
  updated 24 February 2026).
- The files are **not committed**. The federation offers them as a free
  download and states no licence, so the project does not redistribute
  them. They live in `data/wthor/`, which is git-ignored.
- `trainer/wthor.sha256` is committed: one line per file, in the format
  `shasum -a 256` writes, so the files can also be verified from the shell.
  `check` refuses files that do not match. This pins the input, since the
  federation updates the files.
- `data/` is added to `.gitignore`.
- The download is a documented manual step (a `curl` loop in the trainer's
  README section). The trainer does not access the network.

### Format

From the federation's format document (Quin and Nicolet). Integers are
little-endian.

Header, 16 bytes:

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 4 | File creation date |
| 4 | 4 | N1: number of games |
| 8 | 2 | N2: zero in game files |
| 10 | 2 | Year of the games |
| 12 | 1 | P1: board size, 0 or 8 for 8x8 |
| 13 | 1 | P2: 0 for game files |
| 14 | 1 | P3: depth of the theoretical score; 0 means 22 |
| 15 | 1 | Reserved |

Game record, 68 bytes; file length is 16 + 68 × N1:

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 2 | Tournament number |
| 2 | 2 | Black player number |
| 4 | 2 | White player number |
| 6 | 1 | Real score: black's disc count at the end |
| 7 | 1 | Theoretical score: black's disc count under perfect play by both sides from the game's position with P3 empties |
| 8 | 60 | Moves in order, each `column + 10 × row` with a1 = 11 and h8 = 88; 0 after the last move |

- A move byte maps to the engine's square as `(row - 1) × 8 + (column - 1)`.
- Black moves first from the standard start, which is `Board::START`.
- Passes are not recorded. During replay, when the side to move has no
  legal move and the other side has, it passes.
- The current files are stated to carry theoretical scores computed at 24
  empties. The trainer reads P3 and does not assume 24.

### Replay and checks

`check` replays every game with the engine's `play` and reports:

- games read, games replayed to the end, games rejected (an illegal move, or
  a game that stops while moves remain), with counts per reason;
- for games replayed to a terminal position, how many have a real score
  equal to the engine's own result. The engine's `final_score` is the WTHOR
  disc difference, with empties going to the winner; a black count *B* is
  expected to correspond to a difference of `2B − 64`.

**Stop condition.** If fewer than 99% of completed games agree on the real
score, the reading of the format is wrong. Stop and report; do not proceed
to labelling.

The theoretical score cannot be checked by replay, because the engine cannot
solve positions at 24 empties in bulk. Its reading is validated three ways:

- **Format fixtures.** Reader tests built from the format document's own
  statements: P3 gives the empties at which perfect play starts, 0 means 22,
  and the value is black's disc count.
- **Scale agreement.** `check` reports the values of P3 found and the share
  of games whose theoretical score equals the real score. That share is
  substantial only if both fields are black disc counts on one scale.
- **Independent positions.** For at least ten games, the position at P3
  empties is solved exactly by an independent solver and compared with the
  field. This is a one-off check run by hand (Edax is suitable); it is not
  part of the pipeline, and the positions, values and solver are recorded
  in the notes. A disagreement is investigated before labelling; it is not
  assumed to have any one cause.

After labelling, the fit log also reports the correlation between the
theoretical score and the engine's exact value at the game's 14-empties
anchor, both for black. This is a **label-quality diagnostic, not a parser
check.** Ten human moves separate the two, with mistakes by either side, so
a low figure says how noisy the early labels are and nothing about the
format.

Rejected games are skipped. Their number goes in the fit log.

### Positions

A training position is a non-terminal position in which the player to move
has a legal move. Positions where the player must pass are not used; the
search never evaluates them. Each position is stored side-relative, as the
engine sees it.

Repeated positions are kept. The opening moves recur in most games, and the
average of their labels is the right value for them.

### Labels

The raw label, how two humans finished, includes their blunders. Every
position is instead labelled with a perfect-play result from a later point
in its own game:

| Position's empties | Label | Source |
| --- | --- | --- |
| P3 or more | Result of perfect play from the game's position at P3 empties | WTHOR theoretical score, `2T − 64` for black |
| 15 to P3 − 1 | Result of perfect play from the game's first position with 14 or fewer empties | Engine solver |
| 14 or fewer | The position's own exact value | Engine solver |

All labels are converted to the player to move, and from the solver's
centidiscs to the trainer's discs. For a game that ends with more than 14
empties, the final result stands in for the solver's value. A forced pass
changes who is to move without a move being recorded, so the sign of a
label depends on replaying passes correctly; fixtures cover this.

What this removes and what it leaves:

- Removed: every mistake made after the anchor point. Late positions carry
  no label error at all.
- Left: mistakes made between a position and its anchor. A position at 40
  empties is labelled with the truth at 24 empties, after sixteen human
  moves. Over many games this is mostly noise around the right value, with
  some bias toward how humans play.
- Not addressed: humans avoid bad moves, so bad positions are rare in the
  archive, while a search visits them constantly. The cure is positions from
  self-play, which is the next epic and out of scope here.

### The labelling job

- Unit of work: one game. The job solves each of the game's positions with
  14 or fewer empties using `search::solve` with a full window and no node
  limit, and records the values.
- Output: a journal in `data/labels.journal` with one line per game, keyed
  by the game's file and index. It uses the tools crate's journal, so the
  job survives SIGKILL and resumes by re-running the same command. Parallel
  by game with `--jobs`.
- Lines are appended as games finish, so their order depends on scheduling.
  "Identical output" means identical after sorting by key, as in the
  existing journal test. The fit reads the journal into a map, and the
  SHA-256 recorded in the fit log is of the sorted lines.
- Cost: recorded solver medians at 14 empties range from 0.09 to 0.17 s,
  but those were measured through the root search path, not a full-window
  `solve`. Each shallower position costs roughly a third of the one before.
  The working estimate is 10 to 20 CPU-hours, two to four hours on five
  cores. A 1,000-game pilot measures the real rate before the full run, and
  its figure replaces this estimate.
- The first fit is run on the pilot's labels before the full labelling run
  is started. A fault in the label format or the fit then costs minutes,
  not hours.
- The run follows `docs/PLAN.md` § Interruptible long-running jobs,
  including the dedicated worktree and the interrupt-and-resume test.
- The journal is not committed (roughly 10 MB). Its SHA-256 goes in the fit
  log.

## Fitting

Each stage is fitted on its own, from the positions whose empties fall in
it.

- **Held-out games.** One game in twenty is held out, chosen by a hash of
  its move bytes, so the split does not depend on file order. Held-out games
  are never trained on.
- **Example order.** The label journal's line order depends on scheduling.
  The fit sorts games by file and index, and positions within a game by
  ply. Everything below is defined on that order.
- **Shared weights.** As [Shared weights within a
  family](#shared-weights-within-a-family): one learned value per tied pair.
- **The update.** Write `x(i, j)` for the number of the 46 instances in
  position `i` that select entry `j`. It is usually 0 or 1, but can be
  more: at the start position four edges select one entry. With residual
  `r(i)` = answer − guess, computed from the values as they stood at the
  start of the pass, each value moves by

  ```
  step × ( Σ over positions i of  x(i, j) × r(i) )  /  max(n(j), floor)

  n(j) = Σ over positions i of x(i, j)      (instance occurrences)
  ```

  The numerator is the gradient of the squared error and keeps the
  multiplicity. In code this is the plain loop: for each position, for each
  of its 46 features, add the residual to that entry's sum and one to its
  count. `n(j)` counts instance occurrences, not positions; it is used only
  to scale the step.
- **Step, floor and horizon.** Because 46 values share each position's
  residual, the step must be a small fraction of 1/46 for the loop to be
  stable. The step, the floor, the stopping patience and a maximum number
  of passes are constants in the trainer, chosen by held-out error and
  recorded in the fit log with the reason. The floor only damps rare
  entries over this declared horizon, as explained under [How the tables
  are learned](#how-the-tables-are-learned). Explicit regularisation
  (shrinking values toward zero each pass) is added only if the learning
  report shows held-out error rising while training error falls.
- **Stopping.** After each pass the mean absolute error on the held-out
  games is computed. Training stops when it has not improved for the
  patience, or at the maximum, and the best pass's values are kept.
- **Arithmetic and reproducibility.** Training uses `f32` values in discs.
  Floating-point sums depend on the order of addition, so holding the
  values fixed during a pass is not enough for a repeatable result. The
  sums for one stage are accumulated in example order; if a stage is
  parallelised internally, it is cut into fixed chunks by example index and
  the chunk sums are combined in chunk order. The asset is assembled in
  stage order, never completion order. The claim is: the same inputs,
  trainer commit, toolchain, target and build profile give the same bytes.
  The fit log records `rustc --version`, the target triple and the profile.

### Restart

The stages are independent and may be fitted in parallel. The fit must be
restartable under `docs/PLAN.md` § Interruptible long-running jobs.

- **Unit.** One stage, provided a stage fits within the plan's
  bounded-loss rule of a few minutes. The pilot fit measures this first.
  If a stage takes longer, the unit becomes one pass: the checkpoint then
  holds the current values, the best values, the best error and the passes
  since it improved.
- **Stage file.** A finished stage is written to `data/fit/stage_NN` as: a
  header, the tied values, the occurrence counts, and the per-pass learning
  curve. Counts and curve are kept so a resumed run can write the same
  report and `inspect` output as an uninterrupted one.
- **Header.** The layout fingerprint, the label journal's SHA-256, the
  WTHOR manifest's SHA-256, the split rule, the step, floor, patience and
  maximum passes, the table dimensions, and a SHA-256 of the file's body.
- **Atomic publication.** The file is written under a temporary name,
  synced and renamed, as the journal does. A partial file never carries the
  final name.
- **Validation on resume.** A stage file is reused only if its header
  matches the current run exactly and its body checksum verifies. Anything
  else, including a leftover temporary file, is discarded and the stage is
  fitted again.

### The learning report

`fit` writes `docs/tuning/pattern_fit.log`. It is the evidence that learning
happened, separate from whether the engine wins.

- Inputs: trainer commit, toolchain and target, WTHOR manifest and label
  journal SHA-256, layout fingerprint, games used and rejected, positions
  per stage.
- Constants: step, floor, patience, maximum passes.
- Per stage, per pass: mean absolute error in discs on the training and the
  held-out games. This is the learning curve.
- Per stage, final, on held-out games:
  - error of the trained tables, before and after rounding to `i16`;
  - error of always guessing zero;
  - error of the classic evaluator, scaled from centidiscs to discs, and
    its correlation with the labels;
  - the fraction of table values that are non-zero, and the largest
    absolute value.
- The label-quality diagnostic from [Replay and
  checks](#replay-and-checks).
- Asset SHA-256.

Opening positions recur in training and held-out games alike, so held-out
error in the first stages flatters the tables. The report says so.

Low held-out error is necessary and not sufficient. It is measured on
positions humans reached. A search mostly compares positions humans avoid,
and what it needs is the right *ranking* of alternatives, which this error
does not measure directly.

### Reading the tables

`inspect --family edge --stage 6 --count 10` prints the ten highest and ten
lowest configurations of a family at a stage, each drawn on its squares with
its value in discs and its number of occurrences in training. It reads the
stage files in `data/fit/`.

Table values are only determined up to a shift between families (see [How
the tables are learned](#how-the-tables-are-learned)), so `inspect` shows
each value relative to its family's occurrence-weighted mean at that stage,
and says so in its output. Rankings within a family are meaningful; a value
compared across families or stages is not.

## Speed and diagnostics

The pattern evaluator reads about 356 squares per call, where the old one
did far less work. The engine will search fewer positions per second. The
equal-time gate charges it for that, which is why the gate is at equal time
and not equal depth.

Prediction quality and computing cost are separate questions, and the gate
alone cannot tell them apart. These are measured before the gate, reported
in the notes and not gated. Matches here use `bench/openings_tune.txt`,
never the gate's openings.

| Question | Measurement |
| --- | --- |
| What does one evaluation cost? | Nanoseconds per `evaluate` call, each evaluator, over the bench corpus by stage |
| What does that do to the search? | Nodes per second and depth reached at 3 s, over the bench corpus, not only the start position |
| Is the evaluator better at equal effort? | `pattern` against `classic` at equal depth and at equal nodes, 100 pairs each |
| Is it better at equal time? | The gate |

If the new evaluator wins at equal nodes and loses at equal time, the cost
is the problem. If it loses at equal nodes, the tables are the problem
however well they predict. If the search rate falls below half the old
one, improve the indexing within the budget before running the gate.

## Tests

Written first, per the red/green loop.

Engine:

- There are 46 instances and 167,265 table slots, and every `features`
  index lies inside its own family's range.
- Instances are distinct as square sets, and each keeps its ordered list.
- Symmetry: for a position and each of its seven transforms, the multiset
  of *canonical* entries (after the family permutations) is identical.
  With a synthetic table that is tied by those permutations and otherwise
  random, `evaluate` is equal for all eight. With corner 2x5 entries
  deliberately untied the test still passes; with an edge entry untied it
  fails.
- Index convention: a hand-built position gives hand-computed indexes for
  one edge, one corner 2x5 and one diagonal, pinning square order and digit
  significance.
- `stage` at every boundary, and at 64 empties without underflow.
- `evaluate` stays within `±EVAL_BOUND` on the bench corpus.
- Not antisymmetric by contract: no test requires
  `evaluate(b) == -evaluate(pass(b))` of the learned evaluator.
- Asset: exact size, the `assets/` exception in the include guard, the
  layout fingerprint and the fixed-board predictions in
  `assets/eval.layout`.
- The engine's `Cargo.toml` has no dependencies.

Trainer:

- WTHOR reader on a small synthetic file: header fields including P3 and
  its zero case, move decoding, rejection of an illegal move and of a wrong
  file length.
- Replay inserts passes; a known game reaches its known final score.
- Label rule: each of the three rows of the labels table; the sign for the
  player to move in a game containing forced passes, on both sides of the
  pass; centidiscs to discs.
- **Reference problem with multiplicities.** A tiny hand-built set of
  positions in which entries occur zero, one and several times per
  position, with a least-squares prediction worked out by hand. The fit
  must reach that prediction. A fit that counts each position once per
  entry fails this test.
- **Planted-model recovery.** Weights are planted in the two smallest
  families only (diagonals 4 and 5, 324 entries), all others zero.
  Positions are generated so that every planted entry occurs many times,
  labelled from the planted table, and fitted. Held-out predictions must
  match within a stated tolerance. This is the direct test that the loop
  learns; it does not ask the loop to guess entries it never saw.
- Damping over the declared horizon: with the trainer's constants, an entry
  given three noisy examples ends within a stated bound of zero, and an
  unseen entry is exactly zero.
- Tied entries are bit-identical in the written asset.
- Quantisation: a non-finite or out-of-range value fails the fit; trainer
  and engine predictions agree within the stated bound.
- Fit reproducibility: an uninterrupted run and a run killed and resumed
  give identical asset bytes and an identical report.
- Fit restart: a partial temporary file is ignored; a stage file whose
  header does not match the current configuration is refitted; a stage file
  with a damaged body is refitted.
- Labelling: interrupt and resume gives an identical journal after sorting.

## The gate

The opponent is 1.0.0. The engine source today differs from tag `v1.0.0`
only in a doc comment, and this epic does not touch `search.rs`, so the
1.0.0 evaluator running under the current search *is* the 1.0.0 engine. It
is copied into `tools/src/baselines.rs` as the agent `classic[:time]`. A
fingerprint test pins the copy against **golden values frozen from tag
`v1.0.0`**: node counts, best moves and scores on the bench corpus at depth
8, generated once from a checkout of the tag and committed as constants.
Comparing against "the engine's own evaluator" would stop meaning anything
the day the engine's evaluator changes.

Until the gate passes, `SearchConfig::standard()` still uses the old
evaluator, so the candidate needs its own name. The tools gain a second
agent, `pattern[:time]`: the standard search with `pattern::evaluate`.

Pre-registered, one look:

- `pattern:3000` against `classic:3000`.
- 200 pairs, the first 200 openings of `bench/openings_probcut2.txt`. All
  of them were played in the ProbCut gate of issue 117 (at equal nodes, at
  500 ms and, for the first 100, at 3 s). That was a different candidate,
  and nothing in this design was chosen by looking at those games, so the
  set is still a fair test here.
- Four jobs, default load limit, void pairs replayed with the same command
  before any score is read.
- **Pass:** the lower bound of the 95% pair-bootstrap interval of the
  candidate's mean score is above **0.5**. The score runs from 0 to 1, with
  0.5 an even match. This is the same event as the Elo interval's lower
  bound being above 0, which `report` prints beside it. 10,000 resamples,
  seed stated in the pre-registration.
- The pre-registration is committed before the first game. It names the
  asset's SHA-256, the engine commit, the exact command and this rule.
- About three and a half hours.

On a pass: `SearchConfig::standard()` uses the pattern evaluator; the old
evaluator leaves the engine and survives only as the tools copy; the
`pattern` agent name is removed, since `ab` now means the same thing;
`tools/examples/tune.rs`, which tuned the old evaluator, is deleted;
`docs/PLAN.md` records the result.

On a fail: the switch stays off and nothing is deleted. No single cause is
assumed. The evidence is read together: the learning report (including the
classic evaluator's error on the same positions), the equal-depth and
equal-node results, and the cost measurements from [Speed and
diagnostics](#speed-and-diagnostics). Good prediction with a loss at equal
nodes points at ranking positions the archive lacks; a win at equal nodes
with a loss at equal time points at cost; poor prediction can come from the
labels, the representation, the optimisation or the rounding, and the
report's separate figures narrow that down. It comes back for discussion
before more runs.

## Work breakdown

Each line is one issue, in order. The engine stays playable throughout.

1. Trainer crate, budgets and guards: the crate skeleton, its budget test,
   `check.sh`, and the documents brought into line. In `docs/PLAN.md` that
   means the three budgets, plus the two lines saying that assets are
   reproduced "from a `tools/examples` program" and that training pipelines
   belong there uncounted; the trainer crate replaces both. The
   dependencies row of the constraints table is rewritten to match
   [Dependencies](#dependencies), and the engine's no-dependencies guard
   is added.
   The plan's antisymmetry sentence is rewritten as [One point of
   view](#one-point-of-view) describes. `AGENTS.md` and the size note in
   `src/lib.rs` (which still says tools "at most 1,000") change too. The
   golden fingerprint of `v1.0.0` is generated and committed here, before
   anything else moves.
2. WTHOR reader, replay and `check`; the SHA-256 manifest; the download
   instructions; the independent check of the theoretical score.
3. Engine pattern definitions, `features` and `stage`, with tests. No
   weights yet.
4. Label rule and the labelling job, with a 1,000-game pilot. The full run
   waits for issue 5's pilot fit.
5. Fit, restart, learning report and `inspect`, proven on the pilot's
   labels. Then the full labelling run, the full fit, and the asset and
   its layout file committed. Then the include guard exception and
   `pattern::evaluate` land on top.
6. `classic` and `pattern` agents, and the measurements in [Speed and
   diagnostics](#speed-and-diagnostics).
7. Gate pre-registration, run, and the switch or the recorded failure.

## Out of scope

- Self-play data and relabelling by deep search.
- Blending between stages.
- Mobility, parity or other terms beside the tables.
- Incremental index updates.
- Any change to the search, including the return of ProbCut. ProbCut's
  second version waits for this evaluator, as `docs/design/probcut.md`
  records.
- The Phase 2 target of +200 Elo and the comparison with Edax.

## Risks

| Risk | Sign | Response |
| --- | --- | --- |
| Too little data for the 10-square tables | Large gap between training and held-out error | Raise the floor; drop corner 2x5; wider stages |
| Evaluator too slow | Win at equal nodes, loss at equal time | Faster indexing within budget; then incremental updates as a later issue |
| Label noise in early stages | Held-out error barely beats guessing zero before move 20 | Accept for this version; self-play labels are the cure |
| Format misread | Real-score agreement below 99%, or an independently solved position disagrees | Stop and investigate before labelling |
| Tables predict well but rank searched positions badly | Loss at equal nodes despite good held-out error | Self-play positions, the next epic |
| Budget overrun | Evaluator above 230 or trainer above its budget without contortion | Stop and discuss scope, per `AGENTS.md` |

## Review response

Revision 2 answers an external review of revision 1. Every finding was
checked against the code or the mathematics and accepted.

| # | Finding | Change |
| --- | --- | --- |
| 1 | Gate rule gave no units; "above zero" on a score interval would pass a loss | Rule is score lower bound above 0.5, equivalently Elo above 0; asset SHA-256 and rule pinned before the run ([The gate](#the-gate)) |
| 2 | Fit restart unsafe on partial writes and stale inputs | Atomic stage files with a validated header and checksum, counts and curve preserved, unit measured first, restart tests ([Restart](#restart)) |
| 3 | Batch updates do not make `f32` sums order-independent | Stable example order, fixed reduction order, stage-order assembly, stated reproducibility scope ([Fitting](#fitting)) |
| 4 | Update ignored feature multiplicity | Update written with `x(i, j)`; occurrences defined; reference test ([Fitting](#fitting)) |
| 5 | Floor is finite-horizon damping, and the optimum is not unique | Both explanations corrected; horizon declared and tested; `inspect` shows values relative to the family mean |
| 6 | Correlation is not a parser check | Demoted to a label-quality diagnostic; field validated by fixtures, scale agreement and independently solved positions ([Replay and checks](#replay-and-checks)) |
| 7 | "Mirror" too vague; corner 2x5 has no symmetry | Exact permutations per family, direct tying, stronger tests ([Shared weights within a family](#shared-weights-within-a-family)) |
| 8 | Learning the value of the move breaks the antisymmetry contract | Contract change stated; plan and tests updated; pass and unit fixtures added |
| 9 | Asset length alone does not prove compatibility | Layout fingerprint, fixed-board predictions through the loader, explicit quantisation failure ([Asset](#asset-assetsevalbin)) |
| 10 | Failure diagnosis too narrow | Classic evaluator's error reported; equal-depth, equal-node and cost measurements before the gate ([Speed and diagnostics](#speed-and-diagnostics)) |
| — | Golden fingerprint from `v1.0.0` | Adopted |
| — | `stage` underflow | `saturating_sub`, tested |
| — | Planted model must be small and supported | Two smallest families, every entry seen |
| — | Tools 1,300 conflicts with agreed 1,200 | Raised to 1,300, and the trainer to 450, with the user's approval |
| — | Fit pilot before the full labelling run | Adopted in the work breakdown |
