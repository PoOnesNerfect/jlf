mod json;
use std::io::{self, BufRead, IsTerminal, Write};

pub use json::{parse_json, Json, ParseError};

pub fn run() {
    let stdin = io::stdin();

    if stdin.is_terminal() {
        return;
    } else {
        let mut stdout = io::stdout().lock();

        for line in stdin.lock().lines() {
            let line = line.unwrap();
            let json = parse_json(&line).unwrap();

            stdout.write_fmt(format_args!("{json:?}\n")).unwrap();
        }
    }
}
