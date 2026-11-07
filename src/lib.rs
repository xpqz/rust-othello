//! Othello engine conventions.
//!
//! - Score: WTHOR final score in centidiscs; the objective and heuristic-bound
//!   note live in [`score`].
//! - Start: black to move on d5/e4, white on d4/e5; black opens `c4`, `d3`,
//!   `e6`, `f5`.
//! - Perft: a pass consumes one ply; a terminal position with `depth > 0`
//!   counts as one node.
//! - Search: a pass costs no depth; terminal and pass handling precede the
//!   depth cutoff; fail-soft; callers handle pass and terminal roots;
//!   `SearchStatus` is `Complete`, `Partial`, or `Fallback`; fallback is the
//!   first legal move.
//! - Endgame: the solver runs only at the root, at or below the 12-empty
//!   threshold; the default move budget is 1000 ms.
//! - TT: Zobrist keys with the full key stored, strict equal-depth heuristic
//!   reuse, and separate heuristic and endgame domains.
//! - Tools: pair bootstrap with a degeneracy policy, complete frozen
//!   evaluators, and tune/validation/final position sets grouped by the
//!   canonical D4 position after ply 4.
//! - Size: engine plus CLI at most 999 non-comment lines; tools at most 600;
//!   doc comments are excluded.

#![forbid(unsafe_code)]

pub mod agents;
pub mod board;
pub mod cli;
pub mod eval;
pub mod perft;
pub mod rng;
pub mod score;
pub mod search;
pub mod tt;
