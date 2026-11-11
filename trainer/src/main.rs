#![forbid(unsafe_code)]

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("check" | "label" | "fit" | "inspect") => println!("not implemented"),
        _ => {
            eprintln!("usage: othello-trainer check|label|fit|inspect");
            std::process::exit(2);
        }
    }
}
