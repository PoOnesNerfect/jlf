mod builder;
mod fields;
mod input;
mod preview;
mod sample;
mod save;
mod synth;
mod wizard;

use std::io::IsTerminal;

use console::style;
use dialoguer::{theme::ColorfulTheme, Input};

fn main() {
    // Restore the cursor on Ctrl-C: dialoguer hides it (on stderr) during
    // prompts, and a SIGINT would otherwise leave it hidden.
    let _ = ctrlc::set_handler(|| {
        wizard::show_cursor();
        std::process::exit(130);
    });

    // Sample source: explicit file arg > piped stdin > interactive picker.
    let file_arg = std::env::args().nth(1);

    let (sample, run_input) = if let Some(path) = &file_arg {
        match input::from_file(path) {
            Ok(s) => (s, wizard::RunInput::File(path.into())),
            Err(e) => {
                eprintln!("jlf it: cannot read {path}: {e}");
                std::process::exit(1);
            }
        }
    } else if let Some((s, live)) = input::from_stdin_if_piped() {
        let ri = match live {
            Some(l) => wizard::RunInput::Live(l),
            None => wizard::RunInput::Sample,
        };
        (s, ri)
    } else {
        match pick_sample() {
            Some((s, path)) => (s, wizard::RunInput::File(path.into())),
            None => {
                eprintln!(
                    "jlf it: no sample data. Try `jlf it <file>` or `cat logs \
                     | jlf it`."
                );
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
        eprintln!(
            "jlf it: no terminal available for prompts (run it attached to a \
             terminal)."
        );
        std::process::exit(1);
    }

    let jlf = preview::jlf_path();
    if let Err(e) = wizard::run(sample, jlf, run_input) {
        eprintln!("jlf it: {e}");
        std::process::exit(1);
    }
}

/// No file and no pipe: offer the bundled example if present, or ask for a
/// path. Returns the sample text and the file it came from (so "Run it" can
/// re-read it).
fn pick_sample() -> Option<(String, String)> {
    for candidate in ["examples/sample.ndjson", "examples/dummy_logs"] {
        if let Ok(s) = input::from_file(candidate) {
            if !s.trim().is_empty() {
                println!(
                    "{}",
                    style(format!("Using sample data from {candidate}")).dim()
                );
                return Some((s, candidate.to_owned()));
            }
        }
    }
    let theme = ColorfulTheme::default();
    let path: String = Input::with_theme(&theme)
        .with_prompt("Path to a log file to preview against")
        .interact_text()
        .ok()?;
    let path = path.trim().to_owned();
    let s = input::from_file(&path).ok()?;
    Some((s, path))
}
