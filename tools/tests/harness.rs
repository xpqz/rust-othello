use othello::agents::{Agent, greedy_agent, random_agent};
use othello::board::Board;
use othello_tools::harness::{GameResult, pair_score, play_from, play_game, points};

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
