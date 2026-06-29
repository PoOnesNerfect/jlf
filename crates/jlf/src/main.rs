use std::io::{self, Error};

mod cli;

/// Built-in subcommands handled by the core CLI; everything else may dispatch to
/// an external `jlf-<name>` extension.
const BUILTINS: &[&str] = &["expand", "list", "count", "stats", "top", "uniq", "help"];

fn main() {
    maybe_dispatch_extension();

    if let Err(e) = cli::run() {
        if let Some(ioe) = e.root_cause().downcast_ref::<Error>() {
            if ioe.kind() == io::ErrorKind::BrokenPipe {
                // Exit cleanly if the pipe reader disconnected
                std::process::exit(0);
            }
        }
        eprintln!("{e:?}");
    }
}

/// git-style dispatch: if the first argument is a bare word that isn't a built-in,
/// run `jlf-<word>` from PATH with the remaining args (inheriting stdio). If the
/// extension isn't installed, print an install hint and exit.
fn maybe_dispatch_extension() {
    let mut args = std::env::args_os().skip(1);
    let Some(first) = args.next() else { return };
    let sub = first.to_string_lossy();
    if sub.starts_with('-') || BUILTINS.contains(&sub.as_ref()) {
        return;
    }
    // A subcommand is a plain word: no path, operator, or template characters.
    if !sub.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return;
    }

    let bin = format!("jlf-{sub}");
    let rest: Vec<std::ffi::OsString> = args.collect();
    match exec(&bin, &rest) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            eprintln!("jlf: '{sub}' is not a jlf command and `{bin}` is not installed.");
            eprintln!("       install it (e.g. `cargo install {bin}`) or run `jlf help`.");
            std::process::exit(127);
        }
        Err(e) => {
            eprintln!("jlf: failed to run `{bin}`: {e}");
            std::process::exit(126);
        }
    }
}

#[cfg(unix)]
fn exec(bin: &str, args: &[std::ffi::OsString]) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    Err(std::process::Command::new(bin).args(args).exec())
}

#[cfg(not(unix))]
fn exec(bin: &str, args: &[std::ffi::OsString]) -> io::Result<()> {
    let status = std::process::Command::new(bin).args(args).status()?;
    std::process::exit(status.code().unwrap_or(1));
}
