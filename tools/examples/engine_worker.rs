//! One engine, one game: reads `go <me> <opp>` (hex bitboards, side to move
//! first) on stdin and answers with the chosen square. Built against whichever
//! engine source tree it is compiled in. Exits as soon as stdin closes, even
//! in the middle of a search.

use std::io::{BufRead, Write};
use std::sync::mpsc;

use othello::agents::{Agent, search_agent};
use othello::board::Board;
use othello::search::{SearchConfig, SearchLimits};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let time = match args.as_slice() {
        [flag, value] if flag == "--time" => value.parse::<u64>().ok(),
        _ => None,
    };
    let Some(time) = time else {
        eprintln!("usage: engine_worker --time MS");
        std::process::exit(2);
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
        std::process::exit(0);
    });
    // The playing configuration: standard search, one table kept for the game.
    let mut agent = search_agent(SearchConfig::standard(), SearchLimits::new(64, time, 0));
    for line in receiver {
        let mut parts = line.split_whitespace();
        let bits = |text: Option<&str>| text.and_then(|text| u64::from_str_radix(text, 16).ok());
        match (parts.next(), bits(parts.next()), bits(parts.next())) {
            (Some("go"), Some(me), Some(opp)) => {
                let reply = agent
                    .choose(Board { me, opp })
                    .map_or("pass".to_string(), |square| square.to_string());
                println!("{reply}");
                std::io::stdout().flush().unwrap();
            }
            _ => {
                eprintln!("bad command: {line}");
                std::process::exit(2);
            }
        }
    }
}
