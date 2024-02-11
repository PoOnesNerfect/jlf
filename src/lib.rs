use clap::Parser;
use owo_colors::OwoColorize;
use std::io::{self, BufRead, IsTerminal, Write};

pub mod colors;

mod json;
pub use json::{parse_json, Json, ParseError};

mod format;
pub use format::{parse_formatter, Formatter};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(default_value = r#"{#log}\n{spans|data:json}"#)]
    format_string: String,
    /// Disable color output. If output is not a terminal, this is always true
    #[arg(short = 'n', long = "no-color", default_value_t = false)]
    no_color: bool,
    /// Display log in a compact format
    #[arg(short = 'c', long = "compact", default_value_t = false)]
    compact: bool,
    /// If log line is not valid JSON, ignore it instead of reporting error
    #[arg(short = 'l', long = "lenient", default_value_t = false)]
    lenient: bool,
    /// If log line is not valid JSON, print it as is instead of reporting error
    #[arg(short = 'L', long = "super-lenient", default_value_t = false)]
    super_lenient: bool,
}

pub fn run() -> Result<(), color_eyre::Report> {
    color_eyre::install()?;

    let Args {
        format_string,
        no_color,
        compact,
        lenient,
        super_lenient,
    } = Args::parse();

    let stdin = io::stdin();
    if !stdin.is_terminal() {
        let mut stdout = io::stdout().lock();

        let no_color = no_color || !stdout.is_terminal();

        let formatter = parse_formatter(&format_string, no_color, compact)?;

        for line in stdin.lock().lines() {
            let line = line?;
            let json = match parse_json(&line) {
                Ok(json) => json,
                Err(e) => {
                    if super_lenient {
                        stdout.write_fmt(format_args!("{line}\n"))?;
                        continue;
                    }
                    if lenient {
                        continue;
                    }
                    if no_color {
                        stdout.write_fmt(format_args!("{}\n", e))?;
                    } else {
                        stdout.write_fmt(format_args!("{}\n", e.red()))?;
                    }
                    continue;
                }
            };

            let fmt = formatter.with_json(&json);

            stdout.write_fmt(format_args!("{fmt}\n"))?;
        }
    }

    Ok(())
}
