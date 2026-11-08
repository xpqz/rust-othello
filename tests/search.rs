mod common;

use std::time::{Duration, Instant};

use common::playout;
use othello::board::{Board, final_score, is_terminal, legal_moves, pass, play};
use othello::eval::evaluate;
use othello::rng::Rng;
use othello::score::{INF, terminal_score};
use othello::search::{
    Ctx, NO_DEADLINE, SearchConfig, SearchLimits, SearchStatus, negamax, ordered_moves, search,
    search_iterative, search_limited, search_tt,
};
use othello::tt::{Bound, Entry, NO_MOVE, Table, zobrist};

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn sq(file: usize, rank: usize) -> u8 {
    (rank * 8 + file) as u8
}

fn config() -> SearchConfig {
    SearchConfig {
        eval: evaluate,
        endgame_threshold: 12,
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
    assert_eq!(evaluate(child), -6112);
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
    assert_eq!(value, 297);
    assert!(value < 5000);
    let mut ctx = Ctx::new(&config);
    let value = negamax(Board::START, 1, -INF, 0, &mut ctx).unwrap();
    assert_eq!(value, 297);
    assert!(value > 0);
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
    let (order, len) = ordered_moves(board, moves, 1, None);
    assert_eq!(&order[..len], &[sq(0, 0), sq(6, 3), sq(1, 1)]);

    let (order, len) = ordered_moves(board, moves, 1, Some(sq(1, 1)));
    assert_eq!(order[0], sq(1, 1));
    assert_eq!(len, 3);
}

#[test]
fn ordering_start_ties() {
    let board = Board::START;
    let moves = legal_moves(board.me, board.opp);
    let (order, len) = ordered_moves(board, moves, 1, Some(sq(5, 4)));
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
    let (order, len) = ordered_moves(board, moves, 1, None);
    let position = |order: &[u8], len: usize, square: u8| {
        order[..len]
            .iter()
            .position(|&item| item == square)
            .unwrap()
    };
    assert!(position(&order, len, b6) < position(&order, len, c6));
    let (order, len) = ordered_moves(board, moves, 2, None);
    assert!(position(&order, len, c6) < position(&order, len, b6));
}

#[test]
fn ordering_xc_last() {
    let board = Board {
        me: at(3, 3) | at(4, 4),
        opp: at(2, 1) | at(2, 2) | at(2, 3) | at(4, 3) | at(2, 4) | at(3, 4) | at(2, 5),
    };
    let moves = legal_moves(board.me, board.opp);
    let (order, len) = ordered_moves(board, moves, 1, None);
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
    let (order, len) = ordered_moves(board, moves, 1, None);
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
    let board = Board::START;
    let limits = SearchLimits::depth(4);
    let mut table = Table::new(1 << 16);
    let cold = search_tt(board, limits, &config, &mut table);
    let warm = search_tt(board, limits, &config, &mut table);
    assert_eq!(warm.score, cold.score);
    assert!(
        warm.nodes * 2 < cold.nodes,
        "{} vs {}",
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
    let config = config();
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
    let config = config();
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
    let config = config();
    for (board, expected) in corpus() {
        if is_terminal(board) || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        let mut table = Table::new(1 << 16);
        for depth in 1..4 {
            table.store(Entry::heuristic(
                zobrist(board),
                0,
                depth,
                Bound::Exact,
                NO_MOVE,
            ));
        }
        let warm = search_tt(board, SearchLimits::depth(4), &config, &mut table);
        assert_eq!(warm.score, expected, "{board:?}");
    }
}

#[test]
fn corpus_ordering_nodes() {
    let ordered = SearchConfig {
        ordering: true,
        ..config()
    };
    let unordered = SearchConfig {
        ordering: false,
        ..config()
    };
    let (mut with, mut without) = (0, 0);
    for (board, _) in corpus() {
        if is_terminal(board) || legal_moves(board.me, board.opp) == 0 {
            continue;
        }
        with += search_limited(board, SearchLimits::depth(4), &ordered).nodes;
        without += search_limited(board, SearchLimits::depth(4), &unordered).nodes;
    }
    assert!(with * 100 <= without * 60, "{with} vs {without}");
}
