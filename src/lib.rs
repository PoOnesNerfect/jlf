use clap::Parser;
use std::io::{self, BufRead, IsTerminal, Write};

mod json;
pub use json::{parse_json, Json, ParseError};

mod format;
pub use format::{parse_formatter, Formatter};

#[derive(Parser, Debug)]
pub struct Args {
    #[arg(default_value = "{timestamp:fg=green} {level:fg=blue} {message}")]
    format_string: String,
    #[arg(short = 'n', long = "no-color", default_value_t = false)]
    no_color: bool,
}

pub fn run() {
    let Args {
        format_string,
        no_color,
    } = Args::parse();

    let stdin = io::stdin();

    if stdin.is_terminal() {
        return;
    } else {
        let mut stdout = io::stdout().lock();

        let no_color = no_color || !stdout.is_terminal();

        let formatter = parse_formatter(&format_string, no_color).unwrap();

        for line in stdin.lock().lines() {
            let line = line.unwrap();
            let json = parse_json(&line).unwrap();

            let fmt = formatter.with_json(&json);

            stdout.write_fmt(format_args!("{fmt}\n")).unwrap();
        }
    }
}
