mod json;
use std::io::{self, BufRead, IsTerminal};

pub use json::{parse_json, Json, ParseError};

pub fn run() {
    let stdin = io::stdin();

    if stdin.is_terminal() {
        return;
    } else {
        for line in stdin.lock().lines() {
            let line = line.unwrap();
            let json = parse_json(&line).unwrap();
            println!("{:?}", json.get("spans").get_i(2));
        }
    }
}
