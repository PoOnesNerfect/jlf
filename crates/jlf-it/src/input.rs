//! Getting the sample data (a few log lines to preview against) and preparing
//! the terminal so prompts work even when the sample came from a pipe. When the
//! sample is a live pipe we also keep the pipe open so "Run it" can stream the
//! built command against the live tail rather than the finite sample.

use std::fs::File;
use std::io::{BufRead, IsTerminal};
use std::sync::mpsc;
use std::time::Duration;

/// How many lines of sample to keep for previews.
const MAX_SAMPLE: usize = 500;

/// How long to wait for a sample off a pipe before giving up (guards against a
/// live/never-closing stream like `tail -f … | jlf it`).
const PIPE_TIMEOUT: Duration = Duration::from_secs(2);

/// A live input pipe kept open past sampling, so the final "Run it" can stream
/// the built command against it. `leftover` is the bytes the sampler buffered
/// past the sample (emitted before resuming the pipe) so no record is dropped.
pub struct Live {
    pub leftover: Vec<u8>,
    pub pipe: File,
}

/// Read a bounded sample from a reader (one JSON record per line), leaving the
/// reader positioned just past the sample so the caller can keep streaming.
fn read_sample(r: &mut impl BufRead) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for _ in 0..MAX_SAMPLE {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim_end_matches(['\n', '\r']);
                if !trimmed.trim().is_empty() {
                    out.push_str(trimmed);
                    out.push('\n');
                }
            }
            Err(_) => break,
        }
    }
    out
}

/// Load the sample from a file path.
pub fn from_file(path: &str) -> std::io::Result<String> {
    let f = File::open(path)?;
    Ok(read_sample(&mut std::io::BufReader::new(f)))
}

fn timeout_exit() -> ! {
    eprintln!(
        "jlf it: timed out reading a sample from stdin. If it's a live stream \
         (e.g. `tail -f`), pass a finite sample instead — `jlf it <file>` or \
         `head -100 logs | jlf it`."
    );
    std::process::exit(1);
}

/// If stdin is piped (`cat logs | jlf it`), read a bounded sample and then
/// reattach stdin to the controlling terminal so the prompts can read keys.
/// Returns `(sample, live)` if stdin was piped (`live` is the still-open pipe on
/// unix), else `None`.
///
/// The read happens on a thread with a timeout: a finite pipe returns quickly,
/// but a live/idle stream (or a terminal whose tty is misdetected) can't hang
/// the tool forever — it times out with a hint instead.
#[cfg(unix)]
pub fn from_stdin_if_piped() -> Option<(String, Option<Live>)> {
    use std::os::unix::io::FromRawFd;

    if std::io::stdin().is_terminal() {
        return None;
    }
    // Dup the pipe onto our own fd: the sampler and the later live hand-off read
    // it, while fd 0 gets repurposed for the terminal below.
    let raw = unsafe { libc_dup(0) };
    if raw < 0 {
        return Some((sample_stdin_only(), None));
    }
    let file = unsafe { File::from_raw_fd(raw) };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut br = std::io::BufReader::new(file);
        let sample = read_sample(&mut br);
        let leftover = br.buffer().to_vec();
        let pipe = br.into_inner();
        let _ = tx.send((sample, leftover, pipe));
    });
    match rx.recv_timeout(PIPE_TIMEOUT) {
        Ok((sample, leftover, pipe)) => {
            reattach_tty();
            Some((sample, Some(Live { leftover, pipe })))
        }
        Err(_) => timeout_exit(),
    }
}

/// Sample stdin directly with no live hand-off (fallback when the pipe can't be
/// duplicated, and the non-unix path).
fn sample_stdin_only() -> String {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut br = std::io::BufReader::new(std::io::stdin().lock());
        let _ = tx.send(read_sample(&mut br));
    });
    match rx.recv_timeout(PIPE_TIMEOUT) {
        Ok(s) => s,
        Err(_) => timeout_exit(),
    }
}

#[cfg(not(unix))]
pub fn from_stdin_if_piped() -> Option<(String, Option<Live>)> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    Some((sample_stdin_only(), None))
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

#[cfg(unix)]
extern "C" {
    #[link_name = "dup"]
    fn libc_dup(oldfd: i32) -> i32;
    #[link_name = "dup2"]
    fn libc_dup2(oldfd: i32, newfd: i32) -> i32;
}
