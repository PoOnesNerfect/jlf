use std::io::BufRead;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

/// Where the TUI reads records from.
pub enum Source {
    Stdin,
    File(String),
}

/// Spawn a background thread that streams input lines back over a channel, so
/// the UI stays responsive while data keeps arriving (live tail).
///
/// `follow` only affects files: after reaching EOF the reader keeps polling for
/// appended lines. A stdin pipe is "followed" inherently — `read_line` blocks
/// until the producer writes more or closes the pipe.
pub fn spawn(source: Source, follow: bool) -> Receiver<String> {
    let (tx, rx) = channel();
    std::thread::spawn(move || match source {
        Source::Stdin => {
            let stdin = std::io::stdin();
            read_loop(stdin.lock(), &tx, false);
        }
        Source::File(path) => {
            if let Ok(file) = std::fs::File::open(&path) {
                read_loop(std::io::BufReader::new(file), &tx, follow);
            } else {
                let _ = tx.send(format!("jlf-tui: cannot open {path}"));
            }
        }
    });
    rx
}

fn read_loop(mut buf: impl BufRead, tx: &Sender<String>, follow: bool) {
    let mut line = String::new();
    loop {
        line.clear();
        match buf.read_line(&mut line) {
            Ok(0) => {
                if follow {
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                }
                break;
            }
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\n', '\r']);
                if !trimmed.trim().is_empty() && tx.send(trimmed.to_owned()).is_err() {
                    break; // UI dropped the receiver: stop reading.
                }
            }
            Err(_) => break,
        }
    }
}
