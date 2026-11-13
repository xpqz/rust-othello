use othello::board::{
    Board, empty_count, final_score, is_terminal, legal_moves, pass, play, squares,
};
use othello::score::{INF, terminal_score};
use othello::search::{SearchConfig, SearchLimits, search, search_limited, search_tt};
use othello::tt::Table;
use othello_tools::baselines::classic_config;

#[derive(Clone, Copy)]
struct Golden {
    me: u64,
    opp: u64,
    best: u8,
    score: i32,
    nodes: u64,
}

const GOLDEN_V1: [Golden; 23] = [
    Golden {
        me: 0x2002242e10000000,
        opp: 0x023c181028400000,
        best: 26,
        score: -41,
        nodes: 150332,
    },
    Golden {
        me: 0x0008241830000000,
        opp: 0x000402060efe0e00,
        best: 0,
        score: 304,
        nodes: 307098,
    },
    Golden {
        me: 0x003838183c4c8400,
        opp: 0xe0c0c000c0004040,
        best: 39,
        score: -546,
        nodes: 29141,
    },
    Golden {
        me: 0x08602b0801050110,
        opp: 0x101d14161e180a04,
        best: 0,
        score: 347,
        nodes: 298961,
    },
    Golden {
        me: 0x0000106a02f02f70,
        opp: 0x094e2c141c0e0001,
        best: 12,
        score: -365,
        nodes: 389365,
    },
    Golden {
        me: 0x207060d0e77a2160,
        opp: 0x0408142818045e14,
        best: 7,
        score: -117,
        nodes: 35360,
    },
    Golden {
        me: 0x6449b60408d40844,
        opp: 0x883048f87629f010,
        best: 60,
        score: -96,
        nodes: 56047,
    },
    Golden {
        me: 0x000030005f001008,
        opp: 0x0001027c20304010,
        best: 5,
        score: 75,
        nodes: 90670,
    },
    Golden {
        me: 0x012214be502a2000,
        opp: 0x0041220028140a70,
        best: 22,
        score: 138,
        nodes: 206812,
    },
    Golden {
        me: 0x0703071b10080000,
        opp: 0x002010246e76be27,
        best: 24,
        score: 590,
        nodes: 113340,
    },
    Golden {
        me: 0x000813224bc22260,
        opp: 0x82742c1c343c1c1a,
        best: 62,
        score: -550,
        nodes: 121030,
    },
    Golden {
        me: 0x00402414082f5000,
        opp: 0x0010116af4000400,
        best: 43,
        score: -135,
        nodes: 310328,
    },
    Golden {
        me: 0x785c68e1f14f0d11,
        opp: 0x0223171e0e3070a0,
        best: 3,
        score: -2200,
        nodes: 34083,
    },
    Golden {
        me: 0x03e723c0c88c1000,
        opp: 0xd0181c3e37726e7e,
        best: 0,
        score: 1800,
        nodes: 97256,
    },
    Golden {
        me: 0xfff7ea0f8e020040,
        opp: 0x000811f0705cee1d,
        best: 42,
        score: 3800,
        nodes: 5730,
    },
    Golden {
        me: 0x00c8dc1c1c0e44bc,
        opp: 0x3f3723438371bb01,
        best: 6,
        score: -600,
        nodes: 11180,
    },
    Golden {
        me: 0x80c8bc0a04983800,
        opp: 0x390743f55b6747ff,
        best: 31,
        score: -1600,
        nodes: 1886,
    },
    Golden {
        me: 0x0018c47b33050000,
        opp: 0xfe653a84ccfa7f7e,
        best: 55,
        score: 2400,
        nodes: 710,
    },
    Golden {
        me: 0x7c03370918b4f8e4,
        opp: 0x80fcc8f6e64b0612,
        best: 3,
        score: 800,
        nodes: 154,
    },
    Golden {
        me: 0x792307af1f1f0c40,
        opp: 0x80d8f85060e0f33f,
        best: 31,
        score: 200,
        nodes: 88,
    },
    Golden {
        me: 0x0000000000000200,
        opp: 0x0000000000000005,
        best: 255,
        score: -6400,
        nodes: 16,
    },
    Golden {
        me: 0x0000000000000055,
        opp: 0x00000000000000aa,
        best: 255,
        score: 0,
        nodes: 0,
    },
    Golden {
        me: 0x0000000000000001,
        opp: 0x8000000000000000,
        best: 255,
        score: 0,
        nodes: 0,
    },
];

fn config() -> SearchConfig {
    classic_config()
}

fn fingerprint(board: Board, config: &SearchConfig) -> (u8, i32, u64) {
    if is_terminal(board) {
        (255, terminal_score(final_score(board)), 0)
    } else if legal_moves(board.me, board.opp) == 0 {
        let result = search_limited(pass(board), SearchLimits::depth(8), config);
        (255, -result.score, result.nodes)
    } else {
        let result = search_limited(board, SearchLimits::depth(8), config);
        (result.best, result.score, result.nodes)
    }
}

/// `GOLDEN_V1` is the v1.0.0 fingerprint and is never edited. The current
/// search is held to its moves and scores only (a different move must be worth
/// the same); the node counts belong to the v1.0.0 search.
#[test]
fn golden_v1() {
    let config = config();
    for golden in GOLDEN_V1 {
        let board = Board {
            me: golden.me,
            opp: golden.opp,
        };
        let (best, score, _) = fingerprint(board, &config);
        match mover(board) {
            Some(root) if root == board => agrees(
                board,
                8,
                &config,
                (best, score),
                (golden.best, golden.score),
            ),
            _ => assert_eq!((best, score), (golden.best, golden.score), "{board:?}"),
        }
    }
}

/// The standard engine (pattern evaluation) at depth 8 on the corpus. A change
/// that is only meant to be faster leaves move, score and node count as here.
const GOLDEN_V2: [Golden; 23] = [
    Golden {
        me: 0x2002242e10000000,
        opp: 0x023c181028400000,
        best: 26,
        score: -360,
        nodes: 195149,
    },
    Golden {
        me: 0x0008241830000000,
        opp: 0x000402060efe0e00,
        best: 40,
        score: 3657,
        nodes: 238470,
    },
    Golden {
        me: 0x003838183c4c8400,
        opp: 0xe0c0c000c0004040,
        best: 39,
        score: -3603,
        nodes: 24048,
    },
    Golden {
        me: 0x08602b0801050110,
        opp: 0x101d14161e180a04,
        best: 0,
        score: 2527,
        nodes: 129932,
    },
    Golden {
        me: 0x0000106a02f02f70,
        opp: 0x094e2c141c0e0001,
        best: 24,
        score: -1878,
        nodes: 364684,
    },
    Golden {
        me: 0x207060d0e77a2160,
        opp: 0x0408142818045e14,
        best: 7,
        score: -518,
        nodes: 45786,
    },
    Golden {
        me: 0x6449b60408d40844,
        opp: 0x883048f87629f010,
        best: 7,
        score: -326,
        nodes: 57854,
    },
    Golden {
        me: 0x000030005f001008,
        opp: 0x0001027c20304010,
        best: 5,
        score: 1783,
        nodes: 113554,
    },
    Golden {
        me: 0x012214be502a2000,
        opp: 0x0041220028140a70,
        best: 46,
        score: 857,
        nodes: 319098,
    },
    Golden {
        me: 0x0703071b10080000,
        opp: 0x002010246e76be27,
        best: 23,
        score: 3234,
        nodes: 124185,
    },
    Golden {
        me: 0x000813224bc22260,
        opp: 0x82742c1c343c1c1a,
        best: 49,
        score: -3500,
        nodes: 161591,
    },
    Golden {
        me: 0x00402414082f5000,
        opp: 0x0010116af4000400,
        best: 43,
        score: -1303,
        nodes: 246596,
    },
    Golden {
        me: 0x785c68e1f14f0d11,
        opp: 0x0223171e0e3070a0,
        best: 3,
        score: -2200,
        nodes: 14536,
    },
    Golden {
        me: 0x03e723c0c88c1000,
        opp: 0xd0181c3e37726e7e,
        best: 0,
        score: 1800,
        nodes: 40661,
    },
    Golden {
        me: 0xfff7ea0f8e020040,
        opp: 0x000811f0705cee1d,
        best: 42,
        score: 3800,
        nodes: 3371,
    },
    Golden {
        me: 0x00c8dc1c1c0e44bc,
        opp: 0x3f3723438371bb01,
        best: 6,
        score: -600,
        nodes: 6193,
    },
    Golden {
        me: 0x80c8bc0a04983800,
        opp: 0x390743f55b6747ff,
        best: 31,
        score: -1600,
        nodes: 1059,
    },
    Golden {
        me: 0x0018c47b33050000,
        opp: 0xfe653a84ccfa7f7e,
        best: 55,
        score: 2400,
        nodes: 307,
    },
    Golden {
        me: 0x7c03370918b4f8e4,
        opp: 0x80fcc8f6e64b0612,
        best: 3,
        score: 800,
        nodes: 65,
    },
    Golden {
        me: 0x792307af1f1f0c40,
        opp: 0x80d8f85060e0f33f,
        best: 31,
        score: 200,
        nodes: 45,
    },
    Golden {
        me: 0x0000000000000200,
        opp: 0x0000000000000005,
        best: 255,
        score: -6400,
        nodes: 16,
    },
    Golden {
        me: 0x0000000000000055,
        opp: 0x00000000000000aa,
        best: 255,
        score: 0,
        nodes: 0,
    },
    Golden {
        me: 0x0000000000000001,
        opp: 0x8000000000000000,
        best: 255,
        score: 0,
        nodes: 0,
    },
];

#[test]
fn golden_v2() {
    let config = SearchConfig::standard();
    for golden in GOLDEN_V2 {
        let board = Board {
            me: golden.me,
            opp: golden.opp,
        };
        let actual = fingerprint(board, &config);
        assert_eq!(
            actual,
            (golden.best, golden.score, golden.nodes),
            "{board:?}"
        );
    }
}

/// Depth-8 nodes over the corpus before null-window search (engine 039d59a).
const FULL_WINDOW_NODES: u64 = 3_791_452;
/// The same with a cleared table per search (`search_tt`), with null-window
/// search and before shallower table moves were used for ordering.
const TABLE_NODES: u64 = 1_888_096;
/// The same once any heuristic entry's move is tried first, before ordering
/// by evaluation.
const HINT_NODES: u64 = 1_406_429;

#[test]
fn fewer_nodes() {
    let config = SearchConfig::standard();
    let nodes: u64 = GOLDEN_V2
        .iter()
        .map(|golden| {
            let board = Board {
                me: golden.me,
                opp: golden.opp,
            };
            fingerprint(board, &config).2
        })
        .sum();
    assert!(nodes * 10 <= FULL_WINDOW_NODES * 8, "{nodes} nodes");
}

#[test]
fn fewer_table_nodes() {
    let config = SearchConfig::standard();
    let mut table = Table::new(1 << 20);
    let mut nodes = 0;
    for golden in GOLDEN_V2 {
        if let Some(board) = mover(Board {
            me: golden.me,
            opp: golden.opp,
        }) {
            table.clear();
            nodes += search_tt(board, SearchLimits::depth(8), &config, &mut table).nodes;
        }
    }
    assert!(nodes * 100 <= TABLE_NODES * 85, "{nodes} nodes");
    assert!(nodes * 100 <= HINT_NODES * 90, "{nodes} nodes");
}

/// Best move and score of the standard engine at depths 1 to 8 on the corpus
/// (`search_limited`, engine 039d59a, full-window search; a root with no move
/// is searched after the pass). Fixed: later search changes must reproduce
/// every score.
const DEPTH_SCORES: [[(u8, i32); 8]; 23] = [
    [
        (26, 694),
        (26, 439),
        (26, 583),
        (20, -309),
        (21, 352),
        (26, -285),
        (26, 467),
        (26, -360),
    ],
    [
        (40, 4576),
        (0, 4007),
        (40, 4928),
        (40, 3652),
        (40, 4245),
        (40, 3538),
        (32, 4114),
        (40, 3657),
    ],
    [
        (38, -3892),
        (39, -5634),
        (39, -3885),
        (39, -4779),
        (39, -2775),
        (39, -3997),
        (39, -3344),
        (39, -3603),
    ],
    [
        (61, 3533),
        (61, 2040),
        (61, 3246),
        (61, 2519),
        (17, 3171),
        (56, 2440),
        (56, 2883),
        (0, 2527),
    ],
    [
        (12, -1261),
        (12, -1681),
        (12, -916),
        (12, -977),
        (12, -937),
        (12, -1359),
        (29, -1179),
        (24, -1878),
    ],
    [
        (7, 462),
        (7, -852),
        (7, 240),
        (7, -1081),
        (7, -2),
        (7, -1173),
        (7, -60),
        (7, -518),
    ],
    [
        (7, 614),
        (7, 678),
        (7, 946),
        (7, 808),
        (7, -384),
        (7, -601),
        (60, -329),
        (7, -326),
    ],
    [
        (5, 2113),
        (5, 3148),
        (5, 2439),
        (5, 1931),
        (5, 2512),
        (5, 2120),
        (5, 2349),
        (5, 1783),
    ],
    [
        (12, 2072),
        (12, 1144),
        (26, 1618),
        (22, 521),
        (12, 1320),
        (22, 795),
        (46, 1425),
        (46, 857),
    ],
    [
        (24, 4751),
        (24, 4304),
        (16, 4611),
        (23, 3969),
        (23, 4358),
        (23, 3251),
        (31, 4061),
        (23, 3234),
    ],
    [
        (2, -4242),
        (46, -4234),
        (60, -3737),
        (60, -4092),
        (62, -3674),
        (62, -4042),
        (59, -3082),
        (49, -3500),
    ],
    [
        (22, -1214),
        (43, -2304),
        (43, -1078),
        (32, -2030),
        (43, -1130),
        (43, -1871),
        (43, -897),
        (43, -1303),
    ],
    [(3, -2200); 8],
    [(0, 1800); 8],
    [(42, 3800); 8],
    [(6, -600); 8],
    [(31, -1600); 8],
    [(55, 2400); 8],
    [(3, 800); 8],
    [(31, 200); 8],
    [(16, 6400); 8],
    [(255, 0); 8],
    [(255, 0); 8],
];

/// Fail-soft alpha-beta in square order with no table: the independent value.
fn reference(board: Board, depth: u32, mut alpha: i32, beta: i32, eval: fn(Board) -> i32) -> i32 {
    let moves = legal_moves(board.me, board.opp);
    if moves == 0 {
        if is_terminal(board) {
            return terminal_score(final_score(board));
        }
        return -reference(pass(board), depth, -beta, -alpha, eval);
    }
    if depth == 0 {
        return eval(board);
    }
    let mut best = -INF;
    for square in squares(moves) {
        best = best.max(-reference(
            play(board, square),
            depth - 1,
            -beta,
            -alpha,
            eval,
        ));
        alpha = alpha.max(best);
        if alpha >= beta {
            break;
        }
    }
    best
}

/// A result agrees with the record if the score is the same and the move is
/// the recorded one or is worth the same. The child is valued in the root's
/// domain: to the end for a solved root, else at one ply less.
fn agrees(board: Board, depth: u32, config: &SearchConfig, result: (u8, i32), expected: (u8, i32)) {
    assert_eq!(result.1, expected.1, "{board:?} depth {depth}");
    if result.0 != expected.0 {
        assert!(result.0 < 64, "{board:?} depth {depth}: no move");
        let legal = legal_moves(board.me, board.opp) & (1 << result.0) != 0;
        assert!(legal, "{board:?} depth {depth}: illegal move {}", result.0);
        let solved = empty_count(board) <= config.endgame_threshold;
        let below = if solved { 64 } else { depth - 1 };
        let value = -reference(play(board, result.0), below, -INF, INF, config.eval);
        assert_eq!(
            value, expected.1,
            "{board:?} depth {depth} move {}",
            result.0
        );
    }
}

fn mover(board: Board) -> Option<Board> {
    match (is_terminal(board), legal_moves(board.me, board.opp)) {
        (true, _) => None,
        (false, 0) => Some(pass(board)),
        _ => Some(board),
    }
}

/// The no-move sentinel must not pass as square 0, which is the recorded move
/// here; a wrapped shift in a release build would let it.
#[test]
#[should_panic(expected = "no move")]
fn rejects_no_move() {
    let board = Board {
        me: GOLDEN_V2[3].me,
        opp: GOLDEN_V2[3].opp,
    };
    let (best, score) = DEPTH_SCORES[3][7];
    assert_eq!((best, legal_moves(board.me, board.opp) & 1), (0, 1));
    agrees(
        board,
        8,
        &SearchConfig::standard(),
        (64, score),
        (best, score),
    );
}

#[test]
#[cfg_attr(debug_assertions, ignore = "release-only")]
fn corpus_depths() {
    let config = SearchConfig::standard();
    let mut table = Table::new(1 << 20);
    for (golden, row) in GOLDEN_V2.iter().zip(DEPTH_SCORES) {
        let Some(board) = mover(Board {
            me: golden.me,
            opp: golden.opp,
        }) else {
            continue;
        };
        for (depth, expected) in (1..).zip(row) {
            let limits = SearchLimits::depth(depth);
            table.clear();
            for result in [
                search(board, depth, &config),
                search_limited(board, limits, &config),
                search_tt(board, limits, &config, &mut table),
            ] {
                agrees(board, depth, &config, (result.best, result.score), expected);
            }
        }
    }
}
