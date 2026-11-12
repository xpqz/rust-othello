use othello::board::{Board, final_score, is_terminal, legal_moves, pass};
use othello::score::terminal_score;
use othello::search::{SearchConfig, SearchLimits, search_limited};
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

#[test]
fn golden_v1() {
    let config = config();
    for golden in GOLDEN_V1 {
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

/// The standard engine (pattern evaluation) at depth 8 on the corpus, as of
/// issue 133. Speed work must leave move, score and node count unchanged.
const GOLDEN_V2: [Golden; 23] = [
    Golden {
        me: 0x2002242e10000000,
        opp: 0x023c181028400000,
        best: 26,
        score: -360,
        nodes: 188110,
    },
    Golden {
        me: 0x0008241830000000,
        opp: 0x000402060efe0e00,
        best: 40,
        score: 3657,
        nodes: 337639,
    },
    Golden {
        me: 0x003838183c4c8400,
        opp: 0xe0c0c000c0004040,
        best: 39,
        score: -3603,
        nodes: 39862,
    },
    Golden {
        me: 0x08602b0801050110,
        opp: 0x101d14161e180a04,
        best: 0,
        score: 2527,
        nodes: 385013,
    },
    Golden {
        me: 0x0000106a02f02f70,
        opp: 0x094e2c141c0e0001,
        best: 24,
        score: -1878,
        nodes: 441775,
    },
    Golden {
        me: 0x207060d0e77a2160,
        opp: 0x0408142818045e14,
        best: 7,
        score: -518,
        nodes: 70201,
    },
    Golden {
        me: 0x6449b60408d40844,
        opp: 0x883048f87629f010,
        best: 7,
        score: -326,
        nodes: 239787,
    },
    Golden {
        me: 0x000030005f001008,
        opp: 0x0001027c20304010,
        best: 5,
        score: 1783,
        nodes: 157054,
    },
    Golden {
        me: 0x012214be502a2000,
        opp: 0x0041220028140a70,
        best: 46,
        score: 857,
        nodes: 750954,
    },
    Golden {
        me: 0x0703071b10080000,
        opp: 0x002010246e76be27,
        best: 23,
        score: 3234,
        nodes: 149958,
    },
    Golden {
        me: 0x000813224bc22260,
        opp: 0x82742c1c343c1c1a,
        best: 49,
        score: -3500,
        nodes: 210783,
    },
    Golden {
        me: 0x00402414082f5000,
        opp: 0x0010116af4000400,
        best: 43,
        score: -1303,
        nodes: 669213,
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
