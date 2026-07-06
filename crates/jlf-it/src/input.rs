//! Getting the sample data (a few log lines to preview against) and preparing
//! the terminal so prompts work even when the sample came from a pipe.

use std::fs::File;
use std::io::{BufRead, IsTerminal};
use std::sync::mpsc;
use std::time::Duration;

/// How many lines of sample to keep for previews.
const MAX_SAMPLE: usize = 500;

/// How long to wait for a sample off a pipe before giving up (guards against a
/// live/never-closing stream like `tail -f … | jlf it`).
const PIPE_TIMEOUT: Duration = Duration::from_secs(2);

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

/// If stdin is piped (`cat logs | jlf it`), read a bounded sample and then
/// reattach stdin to the controlling terminal so the prompts can read keys.
/// Returns the sample if stdin was piped, else `None`.
///
/// The read happens on a thread with a timeout: a finite pipe returns quickly,
/// but a live/idle stream (or a terminal whose tty is misdetected) can't hang
/// the tool forever — it times out with a hint instead.
pub fn from_stdin_if_piped() -> Option<String> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read_sample(std::io::stdin().lock()));
    });
    match rx.recv_timeout(PIPE_TIMEOUT) {
        Ok(buf) => {
            reattach_tty();
            Some(buf)
        }
        Err(_) => {
            eprintln!(
                "jlf it: timed out reading a sample from stdin. If it's a live \
                 stream (e.g. `tail -f`), pass a finite sample instead — \
                 `jlf it <file>` or `head -100 logs | jlf it`."
            );
            std::process::exit(1);
        }
    }
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
