use core::fmt;
use std::io::{self, BufRead, IsTerminal, Write};

mod json;
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

            let mut log = Log::default();

            for (key, value) in json.as_object().unwrap() {
                match *key {
                    "timestamp" => log.timestamp = value.as_str(),
                    "level" => log.level = value.as_str(),
                    "message" => log.message = value.as_str(),
                    _ => {}
                }
            }

            stdout.write_fmt(format_args!("{log}\n")).unwrap();
        }
    }
}

#[derive(Debug, Default)]
pub struct Log<'a> {
    pub timestamp: Option<&'a str>,
    pub level: Option<&'a str>,
    pub message: Option<&'a str>,
}

impl fmt::Display for Log<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(timestamp) = self.timestamp {
            write!(f, "{} ", timestamp)?;
        }

        if let Some(level) = self.level {
            write!(f, "{} ", level)?;
        }

        if let Some(message) = self.message {
            write!(f, "{}", message)?;
        }

        Ok(())
    }
}
