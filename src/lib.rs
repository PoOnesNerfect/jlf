use std::io::{self, BufRead, IsTerminal, Write};

use clap::Parser;
use owo_colors::OwoColorize;

pub mod colors;

mod json;
pub use json::{parse_json, Json, ParseError};

mod format;
pub use format::{parse_formatter, Formatter};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(default_value = r#"{#log}\n{spans|data|:json}"#)]
    format_string: String,
    /// Disable color output. If output is not a terminal, this is always true
    #[arg(short = 'n', long = "no-color", default_value_t = false)]
    no_color: bool,
    /// Display log with data in a compact format
    #[arg(short = 'c', long = "compact", default_value_t = false)]
    compact: bool,
    /// If log line is not valid JSON, then report it and exit, instead of
    /// printing the line as is
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,
    /// Take only the first N lines
    #[arg(short = 't', long = "take")]
    take: Option<usize>,
}

pub fn run() -> Result<(), color_eyre::Report> {
    color_eyre::install()?;

    let Args {
        format_string,
        no_color,
        compact,
        strict,
        take,
    } = Args::parse();

    let stdin = io::stdin();
    if !stdin.is_terminal() {
        let mut stdout = io::stdout().lock();

        let no_color = no_color || !stdout.is_terminal();

        let formatter = parse_formatter(&format_string, no_color, compact)?;

        let mut buf = stdin.lock();
        let mut line = String::new();
        let mut stripped;
        let mut json = Json::default();
        let mut taken = 0;

        while buf.read_line(&mut line)? != 0 {
            stripped = strip_ansi_escapes::strip_str(&line);

            // keep reference to bypass borrow checker
            // this is safe because we know that line always exists.
            // And, when the line gets cleared, the str slices in json are no
            // longer used.
            let line_ref = unsafe { &*(&stripped as *const String) };

            if let Err(e) = json.parse_replace(line_ref) {
                if strict {
                    if no_color {
                        stdout.write_fmt(format_args!("{:?}\n", e))?;
                    } else {
                        stdout.write_fmt(format_args!("{:?}\n", e.red()))?;
                    }
                    return Ok(());
                }

                if no_color {
                    stdout.write_fmt(format_args!("{stripped}"))?;
                } else {
                    stdout.write_fmt(format_args!("{line}"))?;
                }

                line.clear();
                continue;
            }

            let fmt = formatter.with_json(&json);
            stdout.write_fmt(format_args!("{fmt}\n"))?;

            // clear line to avoid appending
            line.clear();

            // take only N lines if specified
            if let Some(take) = take.as_ref() {
                taken += 1;
                if taken >= *take {
                    break;
                }
            }
        }
    }

    Ok(())
}
