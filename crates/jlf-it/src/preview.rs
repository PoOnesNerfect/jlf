//! Run the real `jlf` binary on the sample to preview a built command.

use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

/// Locate the `jlf` binary: next to this executable first, then on `PATH`.
pub fn jlf_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling =
                dir.join(if cfg!(windows) { "jlf.exe" } else { "jlf" });
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from("jlf")
}

/// Run `jlf <args>` feeding `sample` on stdin; return combined stdout (or the
/// error text). Color is off because stdout is captured (a pipe).
pub fn run(jlf: &std::path::Path, args: &[String], sample: &str) -> String {
    let child = Command::new(jlf)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return format!("(could not run jlf: {e})"),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(sample.as_bytes());
    }
    match child.wait_with_output() {
        Ok(out) => {
            let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
            if s.trim().is_empty() {
                let err = String::from_utf8_lossy(&out.stderr);
                if !err.trim().is_empty() {
                    s = format!("(no output) {}", err.trim());
                } else {
                    s = "(no matching records)".into();
                }
            }
            s
        }
        Err(e) => format!("(jlf failed: {e})"),
    }
}
