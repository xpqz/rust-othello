use othello::agents::{Agent, greedy_agent, random_agent};
use othello::board::Board;
use othello_tools::harness::{GameLog, GameResult, pair_score, play_from, play_game, points};
use othello_tools::positions;
use othello_tools::sym;

fn at(file: usize, rank: usize) -> u64 {
    1u64 << (rank * 8 + file)
}

fn greedy() -> Box<dyn Agent> {
    Box::new(greedy_agent())
}

fn random_factory(seed: u64) -> impl Fn() -> Box<dyn Agent> {
    move || Box::new(random_agent(seed))
}

struct PanicAgent;

impl Agent for PanicAgent {
    fn choose(&mut self, _: Board) -> Option<u8> {
        panic!("agent called")
    }
}

struct H1Agent;

impl Agent for H1Agent {
    fn choose(&mut self, _: Board) -> Option<u8> {
        Some(7)
    }
}

struct NoneAgent;

impl Agent for NoneAgent {
    fn choose(&mut self, _: Board) -> Option<u8> {
        None
    }
}

struct OffBoard;

impl Agent for OffBoard {
    fn choose(&mut self, _: Board) -> Option<u8> {
        Some(64)
    }
}

struct EmptyIllegal;

impl Agent for EmptyIllegal {
    fn choose(&mut self, _: Board) -> Option<u8> {
        Some(0)
    }
}

#[test]
fn games_terminate() {
    let mut black = greedy_agent();
    let mut white = greedy_agent();
    let game = play_game(&[], &mut black, &mut white).unwrap();
    assert!(game.plies > 0);
    assert!(game.score.abs() <= 6400);
}

#[test]
fn opening_replay() {
    let mut first = greedy_agent();
    let mut second = greedy_agent();
    let a = play_game(&[26], &mut first, &mut second).unwrap();
    let mut first = greedy_agent();
    let mut second = greedy_agent();
    let b = play_game(&[26], &mut first, &mut second).unwrap();
    assert_eq!(a, b);
    assert!(play_game(&[0], &mut greedy_agent(), &mut greedy_agent()).is_err());
}

#[test]
fn identical_pairs_draw() {
    let make = greedy;
    assert_eq!(pair_score(&[], &make, &make), Ok(0.5));
    assert_eq!(pair_score(&[26], &make, &make), Ok(0.5));
    let chess = random_factory(9);
    let first = pair_score(&[26], &make, &chess).unwrap();
    let second = pair_score(&[26], &chess, &make).unwrap();
    assert_eq!(first + second, 1.0);
}

#[test]
fn invalid_actions_error() {
    assert!(play_game(&[], &mut NoneAgent, &mut greedy_agent()).is_err());
    assert!(play_game(&[], &mut OffBoard, &mut greedy_agent()).is_err());
    assert!(play_game(&[], &mut EmptyIllegal, &mut greedy_agent()).is_err());
}

#[test]
fn points_map() {
    assert_eq!(points(6200), 1.0);
    assert_eq!(points(0), 0.5);
    assert_eq!(points(-6200), 0.0);
}

#[test]
fn pass_root_passes() {
    let board = Board {
        me: at(0, 0) | at(2, 0) | at(4, 0) | at(6, 0),
        opp: at(1, 0) | at(3, 0) | at(5, 0),
    };
    let game = play_from(board, true, &mut PanicAgent, &mut H1Agent).unwrap();
    assert_eq!(game.passes, 1);
    assert_eq!(game.plies, 2);
    assert_eq!(game.score, -5800);
}

#[test]
fn terminal_root_ends() {
    let board = Board {
        me: at(0, 0),
        opp: at(7, 7),
    };
    let game = play_from(board, true, &mut PanicAgent, &mut PanicAgent).unwrap();
    assert_eq!(game.plies, 0);
    assert_eq!(game.passes, 0);
    assert_eq!(game.score, 0);
}

#[test]
fn passes_occur() {
    let mut passes = 0;
    for seed in 0..50 {
        let mut black = random_agent(seed);
        let mut white = random_agent(seed + 1000);
        let game: GameResult = play_game(&[], &mut black, &mut white).unwrap();
        passes += game.passes;
    }
    assert!(passes > 0);
}

#[test]
fn openings_parse() {
    let text = "c4 e3 f2 c5\n# comment\n\nd3 e3 f5 c3\nc4 e3 f2 c5\n";
    let openings = positions::parse(text).unwrap();
    assert_eq!(openings.len(), 3);
    let stats = positions::stats(&openings);
    assert_eq!(stats.openings, 3);
    assert_eq!(stats.duplicates, 1);
    assert_eq!(stats.lengths, vec![(4, 3)]);
    assert!(positions::parse("c4 e3 f2\n").is_err());
    assert!(positions::parse("a1 e3 f2 c5\n").is_err());
    assert!(positions::parse("c4 e3 f2 c5 x9\n").is_err());
}

#[test]
fn canonical_d4() {
    let key = sym::canonical(Board::START);
    for kind in 0..8 {
        assert_eq!(sym::canonical(sym::transform(Board::START, kind)), key);
    }
}

#[test]
fn split_overlap() {
    let openings =
        positions::parse("c4 e3 f2 c5\nd3 e3 f5 c3\nc4 e3 f4 c3\nf5 f6 d3 g5\n").unwrap();
    let first = openings[0].clone();
    let second = openings[1].clone();
    let third = openings[2].clone();
    let fourth = openings[3].clone();
    assert!(
        positions::check_splits(&[
            std::slice::from_ref(&first),
            std::slice::from_ref(&first),
            std::slice::from_ref(&second)
        ])
        .is_err()
    );
    assert!(positions::check_splits(&[&[second], &[third], &[fourth]]).is_ok());
}

#[test]
fn frozen_sets() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../bench/");
    let tune = positions::load(&format!("{base}openings_tune.txt")).unwrap();
    let validation = positions::load(&format!("{base}openings_validation.txt")).unwrap();
    let final_set = positions::load(&format!("{base}openings_final.txt")).unwrap();
    positions::check_splits(&[&tune, &validation, &final_set]).unwrap();
    for set in [&tune, &validation, &final_set] {
        let stats = positions::stats(set);
        assert_eq!(stats.openings, 512);
        assert_eq!(stats.duplicates, 0);
        assert_eq!(stats.lengths, vec![(8, 512)]);
        let keys: std::collections::HashSet<_> = set.iter().map(|opening| opening.key).collect();
        assert!(keys.len() >= 20, "{} groups", keys.len());
        let prefix: std::collections::HashSet<_> =
            set.iter().take(20).map(|opening| opening.key).collect();
        assert_eq!(prefix.len(), 20);
    }
}

#[test]
fn log_line() {
    let log = GameLog {
        run: 1,
        pair: 0,
        game: 0,
        agent_seeds: (1, 2),
        key: (3, 4),
        opening: "c4 e3 f2 c5".into(),
        black: "random".into(),
        white: "greedy".into(),
        score: 120,
    };
    let line = log.line();
    assert!(line.contains("run 1 pair 0 game 0"), "{line}");
    assert!(line.contains("seeds (1, 2)"), "{line}");
    assert!(line.contains("key (3, 4)"), "{line}");
    assert!(line.contains("opening c4 e3 f2 c5"), "{line}");
    assert!(
        line.contains("black random white greedy score 120"),
        "{line}"
    );
}
