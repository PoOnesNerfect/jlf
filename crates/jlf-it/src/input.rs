//! Getting the sample data (a few log lines to preview against) and preparing
//! the terminal so prompts work even when the sample came from a pipe. When the
//! sample is a live pipe we also keep the pipe open so "Run it" can stream the
//! built command against the live tail rather than the finite sample.

use std::fs::File;
use std::io::{BufRead, IsTerminal};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How many lines of sample to keep for previews.
const MAX_SAMPLE: usize = 500;

/// How long to wait for a sample off a pipe before giving up (guards against a
/// live/never-closing stream like `tail -f … | jlf it` that never emits). Used
/// by the non-unix / fallback sampler.
const PIPE_TIMEOUT: Duration = Duration::from_secs(2);

/// Unix pipe sampler timings: how long to wait for the *first* data, the idle
/// gap that ends a burst (so a live stream that emits then quiets down is used
/// as-is instead of hanging), and the overall cap once data starts flowing.
#[cfg(unix)]
const FIRST_WAIT: Duration = Duration::from_secs(5);
#[cfg(unix)]
const IDLE_GAP: Duration = Duration::from_millis(400);
#[cfg(unix)]
const OVERALL_CAP: Duration = Duration::from_secs(3);

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
/// The sample is read with `poll`, so a finite pipe returns as soon as it ends,
/// while a *live* stream that emits a burst and then goes quiet (a server log,
/// `docker logs -f`, …) is used as soon as it idles — it no longer has to close
/// or fill 500 lines first. Only a stream that emits *nothing* within
/// [`FIRST_WAIT`] gives up with a hint.
#[cfg(unix)]
pub fn from_stdin_if_piped() -> Option<(String, Option<Live>)> {
    use std::os::unix::io::{FromRawFd, RawFd};

    if std::io::stdin().is_terminal() {
        return None;
    }
    // Dup the pipe onto our own fd: the sampler and the later live hand-off read
    // it, while fd 0 gets repurposed for the terminal below.
    let raw: RawFd = unsafe { libc::dup(0) };
    if raw < 0 {
        return Some((sample_stdin_only(), None));
    }

    let (sample, leftover, eof) = sample_pipe(raw);
    // A live stream that never emitted anything: nothing to preview against, and
    // it won't close on its own — point the user at a finite sample.
    if sample.trim().is_empty() && !eof {
        unsafe { libc::close(raw) };
        timeout_exit();
    }

    reattach_tty();
    // SAFETY: `raw` is our own dup of stdin, still open and now blocking again.
    let pipe = unsafe { File::from_raw_fd(raw) };
    Some((sample, Some(Live { leftover, pipe })))
}

/// Poll-read a bounded sample from `fd` without ever blocking indefinitely.
/// Returns `(sample, leftover, eof)` where `leftover` is the bytes read past the
/// last sample line (an incomplete final line, or lines beyond [`MAX_SAMPLE`]) so
/// the live hand-off drops nothing, and `eof` marks a stream that closed.
#[cfg(unix)]
fn sample_pipe(fd: std::os::unix::io::RawFd) -> (String, Vec<u8>, bool) {
    // Non-blocking so reads return EAGAIN instead of hanging; `poll` does the
    // waiting. The original flags are restored before the live hand-off.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags >= 0 {
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    }

    let mut sample = String::new();
    let mut lines = 0usize;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let start = Instant::now();
    let mut last = Instant::now();
    let mut got = false;
    let mut eof = false;

    'outer: while lines < MAX_SAMPLE {
        // How long to wait this round: before any data, up to FIRST_WAIT; after,
        // the shorter of the idle gap (burst ended) and the overall cap.
        let wait = if got {
            IDLE_GAP
                .checked_sub(last.elapsed())
                .zip(OVERALL_CAP.checked_sub(start.elapsed()))
                .map(|(a, b)| a.min(b))
        } else {
            FIRST_WAIT.checked_sub(start.elapsed())
        };
        let Some(wait) = wait else { break };

        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, wait.as_millis().min(i32::MAX as u128) as i32) };
        if r < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        if r == 0 {
            break; // window elapsed: first-data give-up, or burst idle / overall cap
        }
        // Readable (or hung up): drain everything available this round.
        loop {
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n > 0 {
                got = true;
                last = Instant::now();
                buf.extend_from_slice(&chunk[..n as usize]);
                take_lines(&mut buf, &mut sample, &mut lines);
                if lines >= MAX_SAMPLE {
                    break 'outer;
                }
            } else if n == 0 {
                eof = true;
                break 'outer;
            } else {
                break; // EAGAIN / error → done draining for now
            }
        }
    }

    if eof {
        // A finite stream's trailing line without a newline is still a record.
        flush_tail(&mut buf, &mut sample, &mut lines);
    }
    if flags >= 0 {
        unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
    }
    (sample, buf, eof)
}

/// Move every complete (`\n`-terminated) line out of `buf` into `sample`,
/// trimming line endings and skipping blank lines, up to [`MAX_SAMPLE`].
#[cfg(unix)]
fn take_lines(buf: &mut Vec<u8>, sample: &mut String, lines: &mut usize) {
    while *lines < MAX_SAMPLE {
        let Some(pos) = buf.iter().position(|&b| b == b'\n') else {
            break;
        };
        let line: Vec<u8> = buf.drain(..=pos).collect();
        push_line(&line, sample, lines);
    }
}

/// Append `buf`'s remaining bytes as a final line (used on EOF), then clear it.
#[cfg(unix)]
fn flush_tail(buf: &mut Vec<u8>, sample: &mut String, lines: &mut usize) {
    if *lines < MAX_SAMPLE {
        let bytes = std::mem::take(buf);
        push_line(&bytes, sample, lines);
    }
    buf.clear();
}

/// Trim, skip-if-blank, and append one raw line to `sample`.
#[cfg(unix)]
fn push_line(bytes: &[u8], sample: &mut String, lines: &mut usize) {
    let s = String::from_utf8_lossy(bytes);
    let trimmed = s.trim_end_matches(['\n', '\r']);
    if !trimmed.trim().is_empty() {
        sample.push_str(trimmed);
        sample.push('\n');
        *lines += 1;
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
            libc::dup2(tty.as_raw_fd(), 0);
        }
        // keep tty open for the duration of the process
        std::mem::forget(tty);
    }
}

#[cfg(all(unix, test))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::io::FromRawFd;

    fn os_pipe() -> (i32, i32) {
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        (fds[0], fds[1])
    }

    // A live stream that emits a burst then goes quiet (holding the pipe open)
    // is sampled as soon as it idles — it must not hang or return empty.
    #[test]
    fn burst_then_idle_is_sampled() {
        let (rd, wr) = os_pipe();
        let writer = std::thread::spawn(move || {
            let mut w = unsafe { File::from_raw_fd(wr) };
            write!(w, "{{\"a\":1}}\n{{\"a\":2}}\n").unwrap();
            w.flush().unwrap();
            std::thread::sleep(Duration::from_millis(900)); // stay open, idle
        });

        let start = Instant::now();
        let (sample, _leftover, eof) = sample_pipe(rd);
        assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
        assert!(!eof);
        assert_eq!(sample, "{\"a\":1}\n{\"a\":2}\n");

        unsafe { libc::close(rd) };
        writer.join().unwrap();
    }

    // A finite pipe is read through EOF, including a trailing line with no `\n`.
    #[test]
    fn finite_pipe_reads_to_eof() {
        let (rd, wr) = os_pipe();
        {
            let mut w = unsafe { File::from_raw_fd(wr) };
            write!(w, "{{\"a\":1}}\n\n{{\"a\":2}}\ntail-no-newline").unwrap();
        } // wr closed -> EOF

        let (sample, _leftover, eof) = sample_pipe(rd);
        assert!(eof);
        // blank line skipped; trailing newline-less line still captured.
        assert_eq!(sample, "{\"a\":1}\n{\"a\":2}\ntail-no-newline\n");

        unsafe { libc::close(rd) };
    }
}
