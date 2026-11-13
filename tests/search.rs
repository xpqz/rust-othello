mod common;

use std::time::{Duration, Instant};

use common::playout;
use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play, squares};
use othello::pattern::evaluate;
use othello::rng::Rng;
use othello::score::{INF, terminal_score};
use othello::search::{
    Ctx, NO_DEADLINE, SearchConfig, SearchLimits, SearchStatus, negamax, ordered_moves, search,
    search_iterative, search_limited, search_tt, solve, solve_wdl,
};
use othello::tt::{Bound, Domain, Entry, NO_MOVE, Table, zobrist};

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn sq(file: usize, rank: usize) -> u8 {
    (rank * 8 + file) as u8
}

fn heuristic_config() -> SearchConfig {
    SearchConfig {
        endgame_threshold: 0,
        ..config()
    }
}

fn config() -> SearchConfig {
    SearchConfig {
        eval: evaluate,
        endgame_threshold: 12,
        ordering: true,
    }
}

fn minimax(board: Board, depth: u32, config: &SearchConfig) -> i32 {
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if legal_moves(board.opp, board.me) == 0 {
            return terminal_score(final_score(board));
        }
        return -minimax(pass(board), depth, config);
    }
    if depth == 0 {
        return (config.eval)(board);
    }
    let mut best = -INF;
    let mut rest = moves;
    while rest != 0 {
        let square = rest.trailing_zeros() as u8;
        rest &= rest - 1;
        best = best.max(-minimax(play(board, square), depth - 1, config));
    }
    best
}

#[test]
fn depth_zero_terminal() {
    let config = config();
    let board = Board {
        me: at(0, 0) | at(1, 0),
        opp: at(7, 7),
    };
    let mut ctx = Ctx::new(&config);
    assert_eq!(negamax(board, 0, -INF, INF, &mut ctx), Some(6200));
    assert_ne!(evaluate(board), 6200);
}

#[test]
fn horizon_terminal() {
    let config = config();
    let board = Board {
        me: !(at(0, 0) | at(1, 0) | at(7, 7)),
        opp: at(1, 0) | at(7, 7),
    };
    assert_eq!(legal_moves(board.me, board.opp), at(0, 0));
    let child = play(board, sq(0, 0));
    assert!(is_terminal(child));
    assert_eq!(terminal_score(final_score(child)), -6200);
    let mut ctx = Ctx::new(&config);
    assert_eq!(negamax(board, 1, -INF, INF, &mut ctx), Some(6200));
}

#[test]
fn pass_in_tree() {
    let config = config();
    let board = Board {
        me: at(1, 1),
        opp: at(0, 0) | at(2, 0),
    };
    assert_eq!(legal_moves(board.me, board.opp), 0);
    assert!(!is_terminal(board));
    assert_ne!(legal_moves(board.opp, board.me), 0);
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        negamax(board, 2, -INF, INF, &mut ctx),
        Some(minimax(board, 2, &config))
    );
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        negamax(board, 2, -INF, INF, &mut ctx),
        Some(-negamax(pass(board), 2, -INF, INF, &mut Ctx::new(&config)).unwrap())
    );
}

#[test]
fn fail_soft() {
    let config = config();
    let mut ctx = Ctx::new(&config);
    let value = negamax(Board::START, 1, 5000, INF, &mut ctx).unwrap();
    assert_eq!(value, 0);
    assert!(value < 5000);
    let mut ctx = Ctx::new(&config);
    let value = negamax(Board::START, 1, -INF, -100, &mut ctx).unwrap();
    assert_eq!(value, 0);
    assert!(value > -100);
}

#[test]
fn minimax_equivalence() {
    let config = config();
    let mut rng = Rng::new(0x5EA2);
    let mut visited = 0;
    let mut count = 0;
    for _ in 0..400 {
        playout(&mut rng, |board| {
            let sample = visited % 13 == 0 || legal_moves(board.me, board.opp) == 0;
            visited += 1;
            if !sample || count >= 100 {
                return;
            }
            let depth = (count % 4 + 1) as u32;
            let mut ctx = Ctx::new(&config);
            assert_eq!(
                negamax(board, depth, -INF, INF, &mut ctx),
                Some(minimax(board, depth, &config)),
                "{count}: {board:?} depth {depth}"
            );
            count += 1;
        });
        if count >= 100 {
            break;
        }
    }
    assert_eq!(count, 100);
}

fn noise(board: Board) -> i32 {
    ((zobrist(board) >> 20) % 4001) as i32 - 2000
}

/// Ordering, narrowed windows and the table must not change the value:
/// every search entry point equals plain minimax.
#[test]
fn window_equivalence() {
    let config = SearchConfig {
        eval: noise,
        endgame_threshold: 8,
        ordering: true,
    };
    let mut rng = Rng::new(0x135);
    let mut table = Table::new(1 << 16);
    let (mut visited, mut heuristic, mut exact) = (0, 0, 0);
    while heuristic < 1000 || exact < 200 {
        playout(&mut rng, |board| {
            visited += 1;
            if visited % 7 != 0 || legal_moves(board.me, board.opp) == 0 {
                return;
            }
            let solved = board.me.count_ones() + board.opp.count_ones() >= 56;
            let depth = match (solved, heuristic % 50) {
                (true, _) => 64,
                (false, 49) => 5,
                _ => heuristic % 4 + 1,
            };
            if (solved && exact >= 200) || (!solved && heuristic >= 1000) {
                return;
            }
            let expected = minimax(board, depth, &config);
            let label = format!("{board:?} depth {depth}");
            if !solved {
                let mut ctx = Ctx::new(&config);
                let value = negamax(board, depth, -INF, INF, &mut ctx);
                assert_eq!(value, Some(expected), "{label}");
                assert_eq!(search(board, depth, &config).score, expected, "{label}");
            }
            let limits = SearchLimits::depth(depth);
            assert_eq!(
                search_limited(board, limits, &config).score,
                expected,
                "{label}"
            );
            table.clear();
            let warm = search_tt(board, limits, &config, &mut table);
            assert_eq!(warm.score, expected, "{label}");
            let value = -minimax(
                play(board, warm.best),
                depth.saturating_sub(1).min(63),
                &config,
            );
            assert_eq!(value, expected, "best move {label}");
            if solved { exact += 1 } else { heuristic += 1 }
        });
    }
}

/// A narrowed window returns a true bound, and a table filled by narrowed
/// searches does not mislead a full-window search of the same position.
#[test]
fn window_bounds() {
    let config = SearchConfig {
        eval: noise,
        endgame_threshold: 0,
        ordering: true,
    };
    let mut rng = Rng::new(0xB0);
    let (mut visited, mut count) = (0, 0);
    while count < 200 {
        playout(&mut rng, |board| {
            visited += 1;
            if visited % 7 != 0 || count >= 200 || legal_moves(board.me, board.opp) == 0 {
                return;
            }
            let depth = count % 4 + 1;
            let truth = minimax(board, depth, &config);
            let mut table = Table::new(1 << 12);
            let mut ctx = Ctx::new(&config);
            ctx.set_table(&mut table);
            for (alpha, beta) in [
                (truth - 1, truth),
                (truth, truth + 1),
                (truth - 1, truth + 1),
                (truth + 300, truth + 301),
                (truth - 301, truth - 300),
                (-INF, truth),
                (truth, INF),
            ] {
                let value = negamax(board, depth, alpha, beta, &mut ctx).unwrap();
                let label = format!("{board:?} depth {depth} window {alpha} {beta}");
                if value <= alpha {
                    assert!(truth <= value, "{label}: {value} is not an upper bound");
                } else if value >= beta {
                    assert!(truth >= value, "{label}: {value} is not a lower bound");
                } else {
                    assert_eq!(value, truth, "{label}");
                }
            }
            assert_eq!(negamax(board, depth, -INF, INF, &mut ctx), Some(truth));
            count += 1;
        });
    }
}

#[test]
fn caller_roots() {
    let config = config();
    let pass_root = Board {
        me: at(1, 1),
        opp: at(0, 0) | at(2, 0),
    };
    assert_eq!(legal_moves(pass_root.me, pass_root.opp), 0);
    let result = search(pass(pass_root), 2, &config);
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        -result.score,
        negamax(pass_root, 2, -INF, INF, &mut ctx).unwrap()
    );

    let terminal = Board {
        me: at(0, 0),
        opp: at(7, 7),
    };
    assert!(is_terminal(terminal));
    assert_eq!(legal_moves(terminal.me, terminal.opp), 0);
}

#[test]
#[should_panic(expected = "legal root move")]
fn terminal_root_panics() {
    let config = config();
    let terminal = Board {
        me: at(0, 0),
        opp: at(7, 7),
    };
    assert!(is_terminal(terminal));
    let _ = search(terminal, 1, &config);
}

#[test]
fn depth_zero_fallback() {
    let config = config();
    let result = search(Board::START, 0, &config);
    assert_eq!(result.best, sq(3, 2));
    assert_eq!(result.score, evaluate(Board::START));
    assert_eq!(result.depth, 0);
    assert_eq!(result.nodes, 0);
    assert_eq!(result.status, SearchStatus::Fallback);
    assert!(!result.solved);
}

#[test]
fn complete_depth() {
    let config = config();
    let result = search(Board::START, 3, &config);
    let mut ctx = Ctx::new(&config);
    let expected = negamax(Board::START, 3, -INF, INF, &mut ctx).unwrap();
    assert_eq!(result.score, expected);
    assert_eq!(result.depth, 3);
    assert_eq!(result.status, SearchStatus::Complete);
    assert!(!result.solved);
    assert_ne!(
        legal_moves(Board::START.me, Board::START.opp) & (1 << result.best),
        0
    );
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        -negamax(play(Board::START, result.best), 2, -INF, INF, &mut ctx).unwrap(),
        result.score
    );
    assert!(result.nodes > 0);
}

#[test]
fn double_no_move() {
    let config = config();
    let board = Board {
        me: at(0, 0) | at(2, 0) | at(4, 0),
        opp: at(1, 0) | at(3, 0) | at(5, 0) | at(6, 0),
    };
    assert_eq!(legal_moves(board.me, board.opp), at(7, 0));
    let child = play(board, sq(7, 0));
    assert!(is_terminal(child));
    assert_eq!(final_score(child), -60);
    assert_eq!(terminal_score(final_score(child)), -6000);
    let mut ctx = Ctx::new(&config);
    assert_eq!(negamax(board, 1, -INF, INF, &mut ctx), Some(6000));
}

#[test]
fn id_matches_depth() {
    let config = config();
    let boards = [
        Board::START,
        Board {
            me: at(0, 0) | at(2, 0) | at(4, 0),
            opp: at(1, 0) | at(3, 0) | at(5, 0) | at(6, 0),
        },
    ];
    for board in boards {
        for depth in 1..=4 {
            let direct = search(board, depth, &config);
            let iterative = search_iterative(board, depth, &config);
            assert_eq!(iterative.score, direct.score, "{board:?} depth {depth}");
            assert_eq!(iterative.depth, depth);
            assert_eq!(iterative.status, SearchStatus::Complete);
            assert!(!iterative.solved);
            let mut ctx = Ctx::new(&config);
            let value =
                -negamax(play(board, iterative.best), depth - 1, -INF, INF, &mut ctx).unwrap();
            assert_eq!(value, iterative.score, "{board:?} depth {depth}");
        }
    }
}

#[test]
fn id_root_ordering() {
    let config = heuristic_config();
    let mut rng = Rng::new(0x1D);
    let mut positions = Vec::new();
    let mut visited = 0;
    while positions.len() < 30 {
        playout(&mut rng, |board| {
            let sample = visited % 5 == 0;
            visited += 1;
            if sample && positions.len() < 30 && legal_moves(board.me, board.opp).count_ones() >= 2
            {
                positions.push(board);
            }
        });
    }
    let depth = 3;
    let (mut iterative_nodes, mut fixed_nodes) = (0, 0);
    for board in positions {
        let iterative = search_iterative(board, depth, &config);
        let direct = search(board, depth, &config);
        assert_eq!(iterative.score, direct.score, "{board:?}");
        iterative_nodes += iterative.nodes;
        fixed_nodes += (1..=depth)
            .map(|d| search(board, d, &config).nodes)
            .sum::<u64>();
    }
    assert!(
        iterative_nodes < fixed_nodes,
        "{iterative_nodes} vs {fixed_nodes}"
    );
}

#[test]
fn id_zero_is_fallback() {
    let config = config();
    let result = search_iterative(Board::START, 0, &config);
    assert_eq!(result.best, sq(3, 2));
    assert_eq!(result.score, evaluate(Board::START));
    assert_eq!(result.depth, 0);
    assert_eq!(result.status, SearchStatus::Fallback);
}

#[test]
fn deadline_zero_falls_back() {
    let config = config();
    let board = Board::START;
    let limits = SearchLimits {
        max_depth: 10,
        time_ms: 0,
        max_nodes: 0,
    };
    let result = search_limited(board, limits, &config);
    assert_eq!(result.status, SearchStatus::Fallback);
    assert_eq!(result.depth, 0);
    assert!(!result.solved);
    assert_eq!(result.score, evaluate(board));
    assert_ne!(legal_moves(board.me, board.opp) & (1u64 << result.best), 0);
}

#[test]
fn node_limit_partial() {
    let config = config();
    let board = Board::START;
    let depth_one = search(board, 1, &config);
    let limits = SearchLimits {
        max_depth: 10,
        time_ms: NO_DEADLINE,
        max_nodes: depth_one.nodes + 1,
    };
    let result = search_limited(board, limits, &config);
    assert_eq!(result.status, SearchStatus::Partial);
    assert_eq!(result.depth, 1);
    assert_eq!(result.score, depth_one.score);
    assert_eq!(result.best, search_iterative(board, 1, &config).best);
    assert_ne!(legal_moves(board.me, board.opp) & (1u64 << result.best), 0);
    assert!(!result.solved);
}

#[test]
fn node_limit_fallback() {
    let config = config();
    let board = Board::START;
    let limits = SearchLimits {
        max_depth: 10,
        time_ms: NO_DEADLINE,
        max_nodes: 1,
    };
    let result = search_limited(board, limits, &config);
    assert_eq!(result.status, SearchStatus::Fallback);
    assert_eq!(result.depth, 0);
    assert_eq!(result.score, evaluate(board));
    assert_ne!(legal_moves(board.me, board.opp) & (1u64 << result.best), 0);
}

#[test]
fn complete_status() {
    let config = config();
    let board = Board::START;
    let limits = SearchLimits {
        max_depth: 3,
        time_ms: NO_DEADLINE,
        max_nodes: 0,
    };
    let result = search_limited(board, limits, &config);
    assert_eq!(result.status, SearchStatus::Complete);
    assert_eq!(result.depth, 3);
    assert_eq!(result.score, search(board, 3, &config).score);
    assert!(!result.solved);
    assert_ne!(legal_moves(board.me, board.opp) & (1u64 << result.best), 0);
}

#[test]
fn node_limit_boundary() {
    let config = config();
    let board = Board::START;
    let depth_one = search(board, 1, &config);
    let limits = SearchLimits {
        max_depth: 10,
        time_ms: NO_DEADLINE,
        max_nodes: depth_one.nodes,
    };
    let result = search_limited(board, limits, &config);
    assert_eq!(result.status, SearchStatus::Partial);
    assert_eq!(result.depth, 1);
    assert_eq!(result.score, depth_one.score);
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release-only")]
fn deadline_smoke() {
    let config = config();
    let board = Board::START;
    let limits = SearchLimits {
        max_depth: 30,
        time_ms: 50,
        max_nodes: 0,
    };
    let start = Instant::now();
    let result = search_limited(board, limits, &config);
    let elapsed = start.elapsed();
    assert!(elapsed <= Duration::from_millis(100), "{elapsed:?}");
    assert_ne!(result.status, SearchStatus::Complete);
    assert!(!result.solved);
    assert_ne!(legal_moves(board.me, board.opp) & (1u64 << result.best), 0);
}

#[test]
fn ordering_priorities() {
    let board = Board {
        me: at(2, 0) | at(1, 3) | at(4, 3),
        opp: at(1, 0) | at(1, 2) | at(5, 3),
    };
    let moves = legal_moves(board.me, board.opp);
    let (order, len) = ordered_moves(board, moves, 1, None, None);
    assert_eq!(&order[..len], &[sq(0, 0), sq(6, 3), sq(1, 1)]);

    let (order, len) = ordered_moves(board, moves, 1, Some(sq(1, 1)), None);
    assert_eq!(order[0], sq(1, 1));
    assert_eq!(len, 3);
}

#[test]
fn ordering_start_ties() {
    let board = Board::START;
    let moves = legal_moves(board.me, board.opp);
    let (order, len) = ordered_moves(board, moves, 1, Some(sq(5, 4)), None);
    assert_eq!(&order[..len], &[sq(5, 4), sq(3, 2), sq(2, 3), sq(4, 5)]);
}

#[test]
fn ordering_replies() {
    let board = Board {
        me: at(3, 2) | at(3, 3) | at(4, 3),
        opp: at(2, 4) | at(3, 4) | at(4, 4),
    };
    let moves = legal_moves(board.me, board.opp);
    let b6 = sq(1, 5);
    let c6 = sq(2, 5);
    let (order, len) = ordered_moves(board, moves, 1, None, None);
    let position = |order: &[u8], len: usize, square: u8| {
        order[..len]
            .iter()
            .position(|&item| item == square)
            .unwrap()
    };
    assert!(position(&order, len, b6) < position(&order, len, c6));
    let (order, len) = ordered_moves(board, moves, 2, None, None);
    assert!(position(&order, len, c6) < position(&order, len, b6));
}

#[test]
fn ordering_xc_last() {
    let board = Board {
        me: at(3, 3) | at(4, 4),
        opp: at(2, 1) | at(2, 2) | at(2, 3) | at(4, 3) | at(2, 4) | at(3, 4) | at(2, 5),
    };
    let moves = legal_moves(board.me, board.opp);
    let (order, len) = ordered_moves(board, moves, 1, None, None);
    assert_eq!(len, 7);
    assert_eq!(order[len - 1], sq(1, 1));
}

#[test]
fn ordering_c_last() {
    let board = Board {
        me: at(5, 2)
            | at(2, 3)
            | at(3, 3)
            | at(4, 3)
            | at(2, 4)
            | at(3, 4)
            | at(3, 5)
            | at(4, 5)
            | at(3, 6)
            | at(4, 6),
        opp: at(0, 0)
            | at(1, 1)
            | at(3, 1)
            | at(2, 2)
            | at(3, 2)
            | at(6, 2)
            | at(1, 3)
            | at(5, 3)
            | at(0, 4)
            | at(4, 4)
            | at(5, 4)
            | at(6, 4)
            | at(5, 5)
            | at(6, 5)
            | at(2, 6),
    };
    let moves = legal_moves(board.me, board.opp);
    let (order, len) = ordered_moves(board, moves, 1, None, None);
    let position = |square: u8| {
        order[..len]
            .iter()
            .position(|&item| item == square)
            .unwrap()
    };
    assert!(position(sq(7, 1)) > position(sq(0, 2)));
}

#[test]
fn tt_equivalence() {
    let config = config();
    let boards = [
        Board::START,
        Board {
            me: at(0, 0) | at(2, 0) | at(4, 0),
            opp: at(1, 0) | at(3, 0) | at(5, 0) | at(6, 0),
        },
        Board {
            me: at(3, 3) | at(4, 4),
            opp: at(2, 1) | at(2, 2) | at(2, 3) | at(4, 3) | at(2, 4) | at(3, 4) | at(2, 5),
        },
    ];
    for board in boards {
        let limits = SearchLimits::depth(3);
        let cold = search_limited(board, limits, &config);
        let mut table = Table::new(1 << 16);
        let warm = search_tt(board, limits, &config, &mut table);
        assert_eq!(warm.score, cold.score, "{board:?}");
        assert_ne!(legal_moves(board.me, board.opp) & (1u64 << warm.best), 0);
    }
}

#[test]
fn tt_deep_then_shallow() {
    let config = config();
    let board = Board::START;
    let mut table = Table::new(1 << 16);
    let _ = search_tt(board, SearchLimits::depth(4), &config, &mut table);
    let warm = search_tt(board, SearchLimits::depth(3), &config, &mut table);
    let cold = search_limited(board, SearchLimits::depth(3), &config);
    assert_eq!(warm.score, cold.score);
}

#[test]
fn tt_warm_nodes() {
    let config = config();
    let board = Board {
        me: 0x000813224bc22260,
        opp: 0x82742c1c343c1c1a,
    };
    let limits = SearchLimits::depth(8);
    let mut table = Table::new(1 << 16);
    let cold = search_tt(board, limits, &config, &mut table);
    let warm = search_tt(board, limits, &config, &mut table);
    assert_eq!(warm.score, cold.score);
    assert!(
        warm.nodes * 2 < cold.nodes,
        "{} warm vs {} cold",
        warm.nodes,
        cold.nodes
    );
}

#[test]
fn tt_sample_equivalence() {
    let config = config();
    let mut rng = Rng::new(0x1D);
    let mut positions = Vec::new();
    let mut visited = 0;
    while positions.len() < 30 {
        playout(&mut rng, |board| {
            let sample = visited % 5 == 0;
            visited += 1;
            if sample && positions.len() < 30 && legal_moves(board.me, board.opp).count_ones() >= 2
            {
                positions.push(board);
            }
        });
    }
    for board in positions {
        let cold = search_limited(board, SearchLimits::depth(4), &config);
        let mut table = Table::new(1 << 16);
        let tt = search_tt(board, SearchLimits::depth(4), &config, &mut table);
        assert_eq!(tt.score, cold.score, "{board:?}");
        let _ = search_tt(board, SearchLimits::depth(5), &config, &mut table);
        let warm = search_tt(board, SearchLimits::depth(4), &config, &mut table);
        assert_eq!(warm.score, cold.score, "{board:?}");
    }
}

fn corpus() -> Vec<(Board, i32)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/bench/corpus.txt");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let mut parts = line.split_whitespace();
            let me = u64::from_str_radix(parts.next().unwrap(), 16).unwrap();
            let opp = u64::from_str_radix(parts.next().unwrap(), 16).unwrap();
            let score = parts.next().unwrap().parse().unwrap();
            (Board { me, opp }, score)
        })
        .collect()
}

fn corpus_score(board: Board, config: &SearchConfig) -> i32 {
    if is_terminal(board) {
        terminal_score(final_score(board))
    } else if legal_moves(board.me, board.opp) == 0 {
        -search_limited(pass(board), SearchLimits::depth(4), config).score
    } else {
        search_limited(board, SearchLimits::depth(4), config).score
    }
}

#[test]
fn corpus_frozen() {
    let config = heuristic_config();
    let corpus = corpus();
    assert!(corpus.len() >= 20);
    let (mut pass, mut terminal, mut mid, mut end, mut other) = (0, 0, 0, 0, 0);
    for (board, expected) in corpus {
        let empty = othello::board::empty_count(board);
        if is_terminal(board) {
            terminal += 1;
        } else if legal_moves(board.me, board.opp) == 0 {
            pass += 1;
        } else if empty <= 12 {
            end += 1;
        } else if 64 - empty >= 20 {
            mid += 1;
        } else {
            other += 1;
        }
        assert_eq!(corpus_score(board, &config), expected, "{board:?}");
    }
    assert!(pass >= 1 && terminal >= 1 && mid >= 10 && end >= 6 && other == 0);
}

#[test]
fn corpus_tt() {
    let config = heuristic_config();
    for (board, expected) in corpus() {
        if is_terminal(board) {
            continue;
        }
        let root = if legal_moves(board.me, board.opp) == 0 {
            pass(board)
        } else {
            board
        };
        let sign = if root == board { 1 } else { -1 };
        let mut table = Table::new(1 << 16);
        let warm = search_tt(root, SearchLimits::depth(4), &config, &mut table);
        assert_eq!(sign * warm.score, expected, "{board:?}");
    }
}

#[test]
fn corpus_preload() {
    let config = heuristic_config();
    for (board, expected) in corpus() {
        if is_terminal(board) || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let mut table = Table::new(1 << 16);
        for square in squares(legal_moves(board.me, board.opp)) {
            let child = play(board, square);
            let key = zobrist(child);
            table.store(Entry::heuristic(key, -6400, 2, Bound::Exact, NO_MOVE));
        }
        let warm = search_tt(board, SearchLimits::depth(4), &config, &mut table);
        assert_eq!(warm.score, expected, "{board:?}");
    }
}

#[test]
fn corpus_tt_nodes() {
    let config = heuristic_config();
    let (mut cold_nodes, mut tt_nodes) = (0, 0);
    for (board, _) in corpus() {
        if is_terminal(board) || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let limits = SearchLimits::depth(5);
        cold_nodes += search_limited(board, limits, &config).nodes;
        let mut table = Table::new(1 << 16);
        tt_nodes += search_tt(board, limits, &config, &mut table).nodes;
    }
    assert!(
        tt_nodes * 100 <= cold_nodes * 95,
        "{tt_nodes} vs {cold_nodes}"
    );
}

#[test]
fn corpus_abort_store() {
    let config = heuristic_config();
    for (board, expected) in corpus() {
        if is_terminal(board) || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let cold = search_limited(board, SearchLimits::depth(4), &config);
        let mut table = Table::new(1 << 16);
        let abort = SearchLimits {
            max_depth: 4,
            time_ms: NO_DEADLINE,
            max_nodes: 20,
        };
        let _ = search_tt(board, abort, &config, &mut table);
        let warm = search_tt(board, SearchLimits::depth(4), &config, &mut table);
        assert_eq!(warm.score, cold.score, "{board:?}");
        assert_eq!(warm.score, expected, "{board:?}");
    }
}

#[test]
fn corpus_ordering_nodes() {
    let ordered = SearchConfig {
        ordering: true,
        ..heuristic_config()
    };
    let unordered = SearchConfig {
        ordering: false,
        ..heuristic_config()
    };
    let (mut with, mut without) = (0, 0);
    for (board, _) in corpus() {
        if is_terminal(board) || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        with += search_limited(board, SearchLimits::depth(5), &ordered).nodes;
        without += search_limited(board, SearchLimits::depth(5), &unordered).nodes;
    }
    assert!(with * 100 <= without * 60, "{with} vs {without}");
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release-only")]
fn corpus_tt_nodes_deep() {
    let config = heuristic_config();
    let (mut cold_nodes, mut tt_nodes) = (0, 0);
    for (board, _) in corpus() {
        if is_terminal(board) || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let limits = SearchLimits::depth(9);
        cold_nodes += search_limited(board, limits, &config).nodes;
        let mut table = Table::new(1 << 20);
        tt_nodes += search_tt(board, limits, &config, &mut table).nodes;
    }
    assert!(
        tt_nodes * 100 <= cold_nodes * 80,
        "{tt_nodes} vs {cold_nodes}"
    );
}

fn exact(board: Board, alpha: i32, beta: i32) -> i32 {
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if legal_moves(board.opp, board.me) == 0 {
            return terminal_score(final_score(board));
        }
        return -exact(pass(board), -beta, -alpha);
    }
    let mut best = -INF;
    let mut alpha = alpha;
    for square in squares(moves) {
        let value = -exact(play(board, square), -beta, -alpha);
        best = best.max(value);
        alpha = alpha.max(value);
        if alpha >= beta {
            break;
        }
    }
    best
}

fn endgame_board(max_empty: u32) -> Board {
    corpus()
        .into_iter()
        .map(|(board, _)| board)
        .find(|board| {
            !is_terminal(*board)
                && legal_moves(board.me, board.opp) != 0
                && othello::board::empty_count(*board) <= max_empty
        })
        .unwrap()
}

#[test]
fn solve_terminal() {
    let config = config();
    let board = Board {
        me: at(0, 0),
        opp: at(7, 7),
    };
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        solve(board, -INF, INF, &mut ctx),
        Some(terminal_score(final_score(board)))
    );
}

#[test]
fn solve_pass() {
    let config = config();
    let board = Board {
        me: at(1, 1),
        opp: at(0, 0) | at(2, 0),
    };
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        solve(board, -INF, INF, &mut ctx),
        Some(-solve(pass(board), -INF, INF, &mut Ctx::new(&config)).unwrap())
    );
}

#[test]
fn solve_matches_reference() {
    let config = config();
    let board = endgame_board(6);
    let mut ctx = Ctx::new(&config);
    assert_eq!(
        solve(board, -INF, INF, &mut ctx),
        Some(exact(board, -INF, INF))
    );
}

#[test]
fn solver_root_trigger() {
    let config = config();
    let board = endgame_board(6);
    let result = search_limited(board, SearchLimits::depth(10), &config);
    assert_eq!(result.status, SearchStatus::Complete);
    assert_eq!(result.depth, 0);
    assert!(result.solved);
    assert_eq!(result.score, exact(board, -INF, INF));
}

#[test]
fn solver_endgame_domain() {
    let config = config();
    let board = endgame_board(6);
    let mut table = Table::new(1 << 16);
    let _ = search_tt(board, SearchLimits::depth(10), &config, &mut table);
    let entry = table.probe(zobrist(board)).unwrap();
    assert_eq!(entry.domain, Domain::Endgame);
    assert_eq!(entry.depth, 0);
}

fn sample_positions(min_empty: u32, max_empty: u32, count: usize) -> Vec<Board> {
    let mut rng = Rng::new(0x5A_3A_11);
    let mut positions = Vec::new();
    while positions.len() < count {
        playout(&mut rng, |board| {
            let empty = othello::board::empty_count(board);
            if empty >= min_empty && empty <= max_empty && positions.len() < count {
                positions.push(board);
            }
        });
    }
    positions
}

/// Positions with `empties` empty squares: from random games, and the same boards
/// with the discs recoloured evenly and at four lopsided ratios, so that the
/// mover playing, the mover passing, and nobody being able to play all occur
/// often.
fn nearly_full(empties: u32) -> Vec<Board> {
    let mut rng = Rng::new(0x1E);
    let mut boards = Vec::new();
    while boards.len() < 10_000 {
        let mut found = None;
        playout(&mut rng, |board| {
            if othello::board::empty_count(board) == empties {
                found = Some(board);
            }
        });
        let Some(board) = found else { continue };
        let discs = board.me | board.opp;
        let (a, b, c) = (rng.next_u64(), rng.next_u64(), rng.next_u64());
        let (few, most) = (a & b & c & rng.next_u64() & rng.next_u64(), a | b | c);
        boards.push(board);
        for mine in [
            a,
            a & b & c,
            most,
            few,
            most | rng.next_u64() | rng.next_u64(),
        ] {
            boards.push(Board {
                me: discs & mine,
                opp: discs & !mine,
            });
        }
    }
    boards
}

/// The last empty square is settled in the node that meets it, to the exact
/// value whatever window is asked, with no further node.
#[test]
fn last_square() {
    let config = config();
    let mut cases = [0; 3];
    for board in nearly_full(1) {
        let truth = exact(board, -INF, INF);
        let mover = legal_moves(board.me, board.opp) != 0;
        let other = legal_moves(board.opp, board.me) != 0;
        cases[if mover {
            0
        } else if other {
            1
        } else {
            2
        }] += 1;
        for (alpha, beta) in [
            (-INF, INF),
            (truth - 1, truth),
            (truth, truth + 1),
            (-100, 100),
        ] {
            let mut ctx = Ctx::new(&config);
            let value = solve(board, alpha, beta, &mut ctx);
            assert_eq!(value, Some(truth), "{board:?} window {alpha} {beta}");
            assert_eq!(ctx.nodes, 1, "{board:?} window {alpha} {beta}");
        }
    }
    assert!(cases.iter().all(|&count| count > 300), "{cases:?}");
}

/// The last two empty squares are settled in the node that meets them: the
/// exact value for the full window, a true bound outside a narrower one, and
/// no further node.
#[test]
fn last_two_squares() {
    let config = config();
    let mut cases = [0; 3];
    for board in nearly_full(2) {
        let truth = exact(board, -INF, INF);
        let mover = legal_moves(board.me, board.opp) != 0;
        let other = legal_moves(board.opp, board.me) != 0;
        cases[if mover {
            0
        } else if other {
            1
        } else {
            2
        }] += 1;
        for (alpha, beta) in [
            (-INF, INF),
            (truth - 1, truth),
            (truth, truth + 1),
            (-100, 100),
        ] {
            let mut ctx = Ctx::new(&config);
            let value = solve(board, alpha, beta, &mut ctx).unwrap();
            let label = format!("{board:?} window {alpha} {beta}");
            if value <= alpha {
                assert!(truth <= value, "{label}: {value} is not an upper bound");
            } else if value >= beta {
                assert!(truth >= value, "{label}: {value} is not a lower bound");
            } else {
                assert_eq!(value, truth, "{label}");
            }
            assert_eq!(ctx.nodes, 1, "{label}");
        }
    }
    assert!(cases.iter().all(|&count| count > 300), "{cases:?}");
}

#[test]
fn solve_sample() {
    let config = config();
    for board in sample_positions(6, 8, 20) {
        let mut ctx = Ctx::new(&config);
        assert_eq!(
            solve(board, -INF, INF, &mut ctx),
            Some(exact(board, -INF, INF))
        );
    }
}

#[test]
#[ignore = "release-only"]
fn solve_sample_deep() {
    let config = config();
    for board in sample_positions(6, 10, 1000) {
        let mut ctx = Ctx::new(&config);
        assert_eq!(
            solve(board, -INF, INF, &mut ctx),
            Some(exact(board, -INF, INF))
        );
    }
}

#[test]
fn solver_interior_is_heuristic() {
    let base = config();
    let probe = SearchConfig {
        endgame_threshold: 0,
        ..base
    };
    let limits = SearchLimits::depth(3);
    let board = sample_positions(8, 8, 20)
        .into_iter()
        .find(|board| {
            let heuristic = search_limited(*board, limits, &probe);
            let mut ctx = Ctx::new(&base);
            heuristic.score != solve(*board, -INF, INF, &mut ctx).unwrap()
        })
        .expect("fixture where exact and heuristic differ");
    let exact_score = {
        let mut ctx = Ctx::new(&base);
        solve(board, -INF, INF, &mut ctx).unwrap()
    };
    let config = SearchConfig {
        endgame_threshold: 7,
        ..base
    };
    let result = search_limited(board, limits, &config);
    assert_eq!(result.status, SearchStatus::Complete);
    assert!(!result.solved);
    assert_eq!(result.score, search_limited(board, limits, &probe).score);
    assert_ne!(result.score, exact_score);
}

#[test]
fn solver_ignores_heuristic_entries() {
    let config = config();
    let board = endgame_board(6);
    let mut table = Table::new(1 << 16);
    for square in squares(legal_moves(board.me, board.opp)) {
        let key = zobrist(play(board, square));
        for depth in 1..4 {
            table.store(Entry::heuristic(key, 6400, depth, Bound::Exact, NO_MOVE));
            table.store(Entry::heuristic(key, -6400, depth, Bound::Exact, NO_MOVE));
        }
    }
    let result = search_tt(board, SearchLimits::depth(20), &config, &mut table);
    assert!(result.solved);
    assert_eq!(result.score, exact(board, -INF, INF));
}

#[test]
fn heuristic_ignores_endgame_entries() {
    let config = SearchConfig {
        endgame_threshold: 0,
        ..config()
    };
    let board = sample_positions(8, 8, 1)[0];
    let cold = search_limited(board, SearchLimits::depth(3), &config);
    let mut table = Table::new(1 << 16);
    for square in squares(legal_moves(board.me, board.opp)) {
        let key = zobrist(play(board, square));
        table.store(Entry {
            key,
            value: 6400,
            depth: 0,
            flag: Bound::Exact,
            domain: Domain::Endgame,
            mv: NO_MOVE,
        });
    }
    let warm = search_tt(board, SearchLimits::depth(3), &config, &mut table);
    assert_eq!(warm.score, cold.score);
    assert!(!warm.solved);
}

#[test]
fn wdl_sign_matches_exact() {
    let config = config();
    for board in sample_positions(6, 8, 20) {
        let mut ctx = Ctx::new(&config);
        let exact = solve(board, -INF, INF, &mut ctx).unwrap();
        let mut ctx = Ctx::new(&config);
        let wdl = solve_wdl(board, &mut ctx).unwrap();
        assert_eq!(wdl, wdl.signum(), "{board:?}");
        assert_eq!(wdl.signum(), exact.signum(), "{board:?}");
        if exact == 0 {
            assert_eq!(wdl, 0, "{board:?}");
        }
    }
}

#[test]
fn wdl_stores_bounds() {
    let config = config();
    let (mut win, mut loss, mut draw) = (None, None, None);
    for board in sample_positions(6, 8, 100) {
        if win.is_some() && loss.is_some() && draw.is_some() {
            break;
        }
        let mut ctx = Ctx::new(&config);
        let exact = solve(board, -INF, INF, &mut ctx).unwrap();
        match exact.signum() {
            1 if win.is_none() => win = Some(board),
            -1 if loss.is_none() => loss = Some(board),
            0 if draw.is_none() => draw = Some(board),
            _ => {}
        }
    }
    for (board, flag) in [(win.unwrap(), Bound::Lower), (loss.unwrap(), Bound::Upper)] {
        let mut table = Table::new(1 << 16);
        {
            let mut ctx = Ctx::new(&config);
            ctx.set_table(&mut table);
            assert_ne!(solve_wdl(board, &mut ctx).unwrap(), 0);
        }
        let entry = table.probe(zobrist(board)).unwrap();
        assert_eq!(entry.domain, Domain::Endgame);
        assert_eq!(entry.depth, 0);
        assert_eq!(entry.flag, flag);
    }
    let mut table = Table::new(1 << 16);
    {
        let mut ctx = Ctx::new(&config);
        ctx.set_table(&mut table);
        assert_eq!(solve_wdl(draw.unwrap(), &mut ctx), Some(0));
    }
    let entry = table.probe(zobrist(draw.unwrap())).unwrap();
    assert_eq!(entry.flag, Bound::Exact);
    assert_eq!(entry.value, 0);
}

#[test]
fn wdl_cancellation_falls_back() {
    let config = config();
    let board = endgame_board(6);
    let mut table = Table::new(1 << 16);
    let limits = SearchLimits {
        max_depth: 10,
        time_ms: NO_DEADLINE,
        max_nodes: 1,
    };
    let result = search_tt(board, limits, &config, &mut table);
    assert_eq!(result.status, SearchStatus::Fallback);
    assert!(!result.solved);
}

#[test]
fn wdl_widening_matches_exact() {
    let config = config();
    for board in sample_positions(6, 8, 20) {
        let exact = {
            let mut ctx = Ctx::new(&config);
            solve(board, -INF, INF, &mut ctx).unwrap()
        };
        let result = search_limited(board, SearchLimits::depth(10), &config);
        assert_eq!(result.status, SearchStatus::Complete);
        assert!(result.solved);
        assert_eq!(result.depth, 0);
        assert_eq!(result.score, exact, "{board:?}");
    }
}

#[test]
fn wdl_only_partial() {
    let config = config();
    let (board, exact) = sample_positions(6, 8, 100)
        .into_iter()
        .find_map(|board| {
            let mut ctx = Ctx::new(&config);
            let exact = solve(board, -INF, INF, &mut ctx).unwrap();
            (exact != 0).then_some((board, exact))
        })
        .unwrap();
    let mut ctx = Ctx::new(&config);
    let wdl = solve_wdl(board, &mut ctx).unwrap();
    assert_eq!(wdl.signum(), exact.signum());
    let wdl_nodes = ctx.nodes;
    let limits = SearchLimits {
        max_depth: 10,
        time_ms: NO_DEADLINE,
        max_nodes: wdl_nodes + 1,
    };
    let result = search_limited(board, limits, &config);
    assert_eq!(result.status, SearchStatus::Partial);
    assert!(!result.solved);
    assert_eq!(result.depth, 0);
    assert_eq!(result.score.signum(), exact.signum());
    let reply = {
        let mut ctx = Ctx::new(&config);
        solve(play(board, result.best), -INF, INF, &mut ctx).unwrap()
    };
    assert_eq!((-reply).signum(), exact.signum());
}

#[test]
#[ignore = "release-only"]
fn solver_known_endgames() {
    let config = config();
    let known = [
        (0x785c_68e1_f14f_0d11, 0x0223_171e_0e30_70a0, -2200),
        (0x03e7_23c0_c88c_1000, 0xd018_1c3e_3772_6e7e, 1800),
    ];
    for (me, opp, expected) in known {
        let board = Board { me, opp };
        let mut ctx = Ctx::new(&config);
        assert_eq!(
            solve(board, -INF, INF, &mut ctx),
            Some(expected),
            "{board:?}"
        );
        assert_eq!(exact(board, -INF, INF), expected, "{board:?}");
    }
}

#[test]
fn solver_reuses_endgame_bounds() {
    let config = config();
    let (mut cold_nodes, mut warm_nodes) = (0, 0);
    for board in sample_positions(6, 8, 10) {
        let expected = exact(board, -INF, INF);
        let cold = {
            let mut table = Table::new(1 << 16);
            let mut ctx = Ctx::new(&config);
            ctx.set_table(&mut table);
            let score = solve(board, -INF, INF, &mut ctx);
            assert_eq!(score, Some(expected), "{board:?}");
            ctx.nodes
        };
        let mut table = Table::new(1 << 16);
        {
            let mut ctx = Ctx::new(&config);
            ctx.set_table(&mut table);
            solve_wdl(board, &mut ctx).unwrap();
        }
        let warm = {
            let mut ctx = Ctx::new(&config);
            ctx.set_table(&mut table);
            let score = solve(board, -INF, INF, &mut ctx);
            assert_eq!(score, Some(expected), "{board:?}");
            ctx.nodes
        };
        cold_nodes += cold;
        warm_nodes += warm;
    }
    assert!(warm_nodes < cold_nodes, "{warm_nodes} vs {cold_nodes}");
}

fn percentile(times: &[f64], fraction: f64) -> f64 {
    let index = ((times.len() - 1) as f64 * fraction).round() as usize;
    times[index]
}

#[test]
#[ignore = "release-only"]
fn endgame_bench() {
    for (remaining, count) in [(12, 20), (14, 10)] {
        let config = SearchConfig {
            endgame_threshold: remaining,
            ..config()
        };
        let mut boards = sample_positions(remaining, remaining, count);
        if remaining == 12 {
            boards.push(Board {
                me: 0x785c_68e1_f14f_0d11,
                opp: 0x0223_171e_0e30_70a0,
            });
        }
        let mut times = Vec::new();
        let mut nodes = 0;
        let mut completed = 0;
        for board in boards {
            if legal_moves(board.me, board.opp) == 0 {
                continue;
            }
            let mut table = Table::new(othello::tt::MAX_ENTRIES);
            let limit = if remaining == 14 { 3000 } else { 1000 };
            let start = Instant::now();
            let result = search_tt(board, SearchLimits::new(64, limit, 0), &config, &mut table);
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            nodes += result.nodes;
            completed += result.solved as usize;
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = percentile(&times, 0.5);
        let p90 = percentile(&times, 0.9);
        let p99 = percentile(&times, 0.99);
        let nps = nodes as f64 * 1000.0 / times.iter().sum::<f64>().max(1e-9);
        println!(
            "{remaining} empty: n={} completed={} median={median:.2}ms p90={p90:.2}ms p99={p99:.2}ms nps={nps:.0}",
            times.len(),
            completed
        );
        if remaining == 12 {
            assert!(median < 1000.0, "{median}");
        }
        if remaining == 14 {
            let mut partial = (0usize, 0usize, 0.0f64);
            let mut fallback = (0usize, 0usize, 0.0f64);
            for board in sample_positions(14, 14, count) {
                if legal_moves(board.me, board.opp) == 0 {
                    continue;
                }
                let mut ctx = Ctx::new(&config);
                let exact = solve(board, -INF, INF, &mut ctx).unwrap();
                let mut table = Table::new(othello::tt::MAX_ENTRIES);
                let result = search_tt(board, SearchLimits::new(64, 5, 0), &config, &mut table);
                let reply = {
                    let mut ctx = Ctx::new(&config);
                    solve(play(board, result.best), -INF, INF, &mut ctx).unwrap()
                };
                let value = -reply;
                let kept = usize::from(value.signum() == exact.signum());
                let loss = (exact - value) as f64;
                let bucket = match result.status {
                    SearchStatus::Partial => &mut partial,
                    SearchStatus::Fallback => &mut fallback,
                    SearchStatus::Complete => continue,
                };
                bucket.0 += 1;
                bucket.1 += kept;
                bucket.2 += loss;
            }
            println!(
                "14 interrupted: partial n={} kept={} avg_loss={:.1} fallback n={} kept={} avg_loss={:.1}",
                partial.0,
                partial.1,
                partial.2 / partial.0.max(1) as f64,
                fallback.0,
                fallback.1,
                fallback.2 / fallback.0.max(1) as f64
            );
            assert_eq!(partial.1, partial.0, "Partial lost a proven WDL result");
        }
    }
}

#[test]
#[ignore = "release-only"]
fn solver_full_game() {
    let config = config();
    let mut board = Board::START;
    let mut elapsed = 0.0;
    let mut slowest: f64 = 0.0;
    let mut solved = 0;
    let mut moves = 0;
    let mut table = Table::new(othello::tt::MAX_ENTRIES);
    while !is_terminal(board) {
        let legal = legal_moves(board.me, board.opp);
        if legal == 0 {
            board = pass(board);
            continue;
        }
        let start = Instant::now();
        let result = search_tt(board, SearchLimits::new(64, 200, 0), &config, &mut table);
        let move_ms = start.elapsed().as_secs_f64() * 1000.0;
        elapsed += move_ms;
        slowest = slowest.max(move_ms);
        assert!(legal & (1 << result.best) != 0, "{board:?}");
        solved += result.solved as usize;
        board = play(board, result.best);
        moves += 1;
    }
    println!(
        "full game: moves={moves} solved={solved} limit=200ms elapsed={elapsed:.2}ms slowest={slowest:.2}ms"
    );
    assert!(moves > 0);
    assert!(solved > 0, "solver never completed an endgame");
    assert!(slowest <= 250.0, "{slowest}");
}

#[test]
fn standard_threshold_is_14() {
    assert_eq!(SearchConfig::standard().endgame_threshold, 14);
}
