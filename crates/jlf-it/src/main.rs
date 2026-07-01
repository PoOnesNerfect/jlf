mod builder;
mod input;
mod preview;
mod save;
mod wizard;

use std::io::IsTerminal;

use console::style;
use dialoguer::{theme::ColorfulTheme, Input};

fn main() {
    // Sample source: explicit file arg > piped stdin > interactive picker.
    let file_arg = std::env::args().nth(1);

    let sample = if let Some(path) = &file_arg {
        match input::from_file(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("jlf it: cannot read {path}: {e}");
                std::process::exit(1);
            }
        }
    } else if let Some(piped) = input::from_stdin_if_piped() {
        piped
    } else {
        match pick_sample() {
            Some(s) => s,
            None => {
                eprintln!("jlf it: no sample data. Try `jlf it <file>` or `cat logs | jlf it`.");
                std::process::exit(1);
            }
        }
    };

    if sample.trim().is_empty() {
        eprintln!("jlf it: the sample is empty — nothing to preview against.");
        std::process::exit(1);
    }

    // After a possible pipe drain, we need a terminal for the prompts.
    if !std::io::stdin().is_terminal() {
        eprintln!("jlf it: no terminal available for prompts (run it attached to a terminal).");
        std::process::exit(1);
    }

    let jlf = preview::jlf_path();
    if let Err(e) = wizard::run(sample, jlf) {
        eprintln!("jlf it: {e}");
        std::process::exit(1);
    }
}

/// No file and no pipe: offer the bundled example if present, or ask for a path.
fn pick_sample() -> Option<String> {
    for candidate in ["examples/sample.ndjson", "examples/dummy_logs"] {
        if let Ok(s) = input::from_file(candidate) {
            if !s.trim().is_empty() {
                println!(
                    "{}",
                    style(format!("Using sample data from {candidate}")).dim()
                );
                return Some(s);
            }
        }
    }
    let theme = ColorfulTheme::default();
    let path: String = Input::with_theme(&theme)
        .with_prompt("Path to a log file to preview against")
        .interact_text()
        .ok()?;
    input::from_file(path.trim()).ok()
}
