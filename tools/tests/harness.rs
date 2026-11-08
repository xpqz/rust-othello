use othello::agents::{Agent, greedy_agent, random_agent};
use othello::board::Board;
use othello_tools::harness::{GameResult, pair_score, play_game};

fn greedy() -> Box<dyn Agent> {
    Box::new(greedy_agent())
}

fn random_factory(seed: u64) -> impl Fn() -> Box<dyn Agent> {
    move || Box::new(random_agent(seed))
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
    assert_eq!(play_game(&[0], &mut greedy_agent(), &mut greedy_agent()), Err("illegal opening move".to_string()));
}

#[test]
fn identical_pairs_draw() {
    let make = greedy;
    assert_eq!(pair_score(&[], &make, &make), Ok(0.5));
    assert_eq!(pair_score(&[26], &make, &make), Ok(0.5));
    let chess = random_factory(9);
    assert_eq!(pair_score(&[26], &make, &chess) + pair_score(&[26], &chess, &make), Ok(1.0));
}

#[test]
fn invalid_actions_error() {
    assert!(play_game(&[], &mut NoneAgent, &mut greedy_agent()).is_err());
    assert!(play_game(&[], &mut OffBoard, &mut greedy_agent()).is_err());
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
