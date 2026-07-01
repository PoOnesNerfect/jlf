//! Getting the sample data (a few log lines to preview against) and preparing
//! the terminal so prompts work even when the sample came from a pipe.

use std::fs::File;
use std::io::{BufRead, IsTerminal, Read};

/// How many lines of sample to keep for previews.
const MAX_SAMPLE: usize = 500;

/// Read a bounded sample from a reader (one JSON record per line).
fn read_sample(r: impl BufRead) -> String {
    let mut out = String::new();
    for (i, line) in r.lines().enumerate() {
        if i >= MAX_SAMPLE {
            break;
        }
        let Ok(line) = line else { break };
        if !line.trim().is_empty() {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// Load the sample from a file path.
pub fn from_file(path: &str) -> std::io::Result<String> {
    let f = File::open(path)?;
    Ok(read_sample(std::io::BufReader::new(f)))
}

/// If stdin is piped (`cat logs | jlf it`), drain it as the sample and then
/// reattach stdin to the controlling terminal so the prompts can read keys.
/// Returns the sample if stdin was piped, else `None`.
pub fn from_stdin_if_piped() -> Option<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut buf = String::new();
    let mut lock = stdin.lock();
    // Read the whole pipe (bounded lazily by the reader below via take-like loop).
    let mut raw = String::new();
    if lock.read_to_string(&mut raw).is_ok() {
        buf = read_sample(std::io::Cursor::new(raw));
    }
    reattach_tty();
    Some(buf)
}

/// Reattach fd 0 (stdin) to /dev/tty so interactive prompts work after the
/// original stdin (a pipe) was consumed. Best-effort; no-op on non-unix.
#[cfg(unix)]
fn reattach_tty() {
    use std::os::unix::io::AsRawFd;
    if let Ok(tty) = File::open("/dev/tty") {
        // SAFETY: dup2 onto fd 0 is a standard reattach; both fds are valid.
        unsafe {
            libc_dup2(tty.as_raw_fd(), 0);
        }
        // keep tty open for the duration of the process
        std::mem::forget(tty);
    }
}

#[cfg(not(unix))]
fn reattach_tty() {}

#[cfg(unix)]
extern "C" {
    #[link_name = "dup2"]
    fn libc_dup2(oldfd: i32, newfd: i32) -> i32;
}
