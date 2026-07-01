//! The interactive prompt flow: assemble a [`Builder`], preview it, then let the
//! user run and/or save it.

use std::path::PathBuf;

use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

use crate::builder::{Builder, Mode};
use crate::{preview, save};

/// Outcome of a prompt: a value, or the user asked to quit (Esc/Ctrl-C).
type Prompt<T> = Result<T, ()>;

fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

fn select(prompt: &str, items: &[&str]) -> Prompt<usize> {
    let theme = theme();
    Select::with_theme(&theme)
        .with_prompt(prompt)
        .items(items)
        .default(0)
        .interact_opt()
        .map_err(|_| ())?
        .ok_or(())
}

fn input(prompt: &str, default: &str) -> Prompt<String> {
    let theme = theme();
    let mut b = Input::<String>::with_theme(&theme)
        .with_prompt(prompt)
        .allow_empty(true);
    if !default.is_empty() {
        b = b.default(default.to_owned());
    }
    b.interact_text().map_err(|_| ())
}

fn confirm(prompt: &str, default: bool) -> Prompt<bool> {
    let theme = theme();
    Confirm::with_theme(&theme)
        .with_prompt(prompt)
        .default(default)
        .interact()
        .map_err(|_| ())
}

fn split_commas(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Build the command through prompts. Returns `None` if the user quit.
fn build() -> Option<Builder> {
    let mut b = Builder::default();

    let mode = match select(
        "What do you want to build?",
        &[
            "View — pretty-print / pick fields",
            "Summarize — count / stats / top / uniq",
            "Export — a csv / tsv / md table",
        ],
    ) {
        Ok(0) => Mode::View,
        Ok(1) => Mode::Summarize,
        Ok(2) => Mode::Export,
        _ => return None,
    };

    let filters = input("Filters? (e.g. `level=error status>=500`, blank to skip)", "").ok()?;
    b.filters = filters.split_whitespace().map(str::to_owned).collect();

    match mode {
        Mode::View => {
            let tmpl = input(
                "Template or fields? (`{ts} {msg}` for a template, `ts,level` for fields, blank = default)",
                "",
            )
            .ok()?;
            if tmpl.contains('{') {
                b.template = Some(tmpl);
            } else if !tmpl.trim().is_empty() {
                b.fields = split_commas(&tmpl);
            }
            b.compact = confirm("Compact (one line per record)?", false).ok()?;
            let redact = input("Redact fields? (comma globs like `token,*.email`, blank to skip)", "").ok()?;
            b.redact = split_commas(&redact);
        }
        Mode::Summarize => {
            let verb = match select("Which summary?", &["count", "stats", "top", "uniq"]) {
                Ok(0) => "count",
                Ok(1) => "stats",
                Ok(2) => "top",
                Ok(3) => "uniq",
                _ => return None,
            };
            b.verb = Some(verb.to_owned());
            let field_prompt = if verb == "count" {
                "Field to break down by? (blank = just the total)"
            } else {
                "Field?"
            };
            let field = input(field_prompt, "").ok()?;
            if !field.trim().is_empty() {
                b.field = Some(field.trim().to_owned());
            }
            if verb == "top" {
                let n = input("How many (top N)?", "10").ok()?;
                b.n = n.trim().parse().ok();
            }
            let by = input("Group by a field? (blank to skip)", "").ok()?;
            if !by.trim().is_empty() {
                b.by = Some(by.trim().to_owned());
            }
            match select("Output as?", &["text", "csv", "tsv", "md"]) {
                Ok(0) | Err(()) => {}
                Ok(i) => b.format = Some(["", "csv", "tsv", "md"][i].to_owned()),
            }
        }
        Mode::Export => {
            let fmt = match select("Table format?", &["csv", "tsv", "md"]) {
                Ok(i) => ["csv", "tsv", "md"][i],
                Err(()) => return None,
            };
            b.format = Some(fmt.to_owned());
            let cols = input("Columns? (comma-separated, e.g. `ts,level,msg`)", "").ok()?;
            b.fields = split_commas(&cols);
        }
    }

    Some(b)
}

/// Run the whole wizard against `sample`, using `jlf` for previews/runs.
pub fn run(sample: String, jlf: PathBuf) -> std::io::Result<()> {
    let term = console::Term::stdout();
    let _ = term.clear_screen();
    println!("{}", style(" jlf it — build a command ").black().on_cyan());
    println!(
        "{}\n",
        style("Answer a few prompts; you'll see a live preview, then run or save it.").dim()
    );

    let builder = loop {
        let Some(b) = build() else {
            println!("\n{}", style("cancelled").dim());
            return Ok(());
        };

        // Preview
        println!("\n{}", style("── preview ──────────────").dim());
        println!("{}", style(b.command_line()).cyan());
        println!();
        let out = preview::run(&jlf, &b.to_args(), &sample);
        for line in out.lines().take(15) {
            println!("  {line}");
        }
        if out.lines().count() > 15 {
            println!("  {}", style("…").dim());
        }
        println!("{}\n", style("─────────────────────────").dim());

        match select("Happy with this?", &["Yes — continue", "No — start over", "Quit"]) {
            Ok(0) => break b,
            Ok(1) => continue,
            _ => return Ok(()),
        }
    };

    // Final action
    let action = match select(
        "What now?",
        &["Run it now", "Save as a recipe", "Save and run", "Just print the command"],
    ) {
        Ok(i) => i,
        Err(()) => return Ok(()),
    };

    if matches!(action, 1 | 2) {
        save_flow(&builder)?;
    }
    if matches!(action, 0 | 2) {
        println!("\n{}", style("── output ───────────────").dim());
        print!("{}", preview::run(&jlf, &builder.to_args(), &sample));
        println!("{}", style("─────────────────────────").dim());
    }
    if action == 3 {
        println!("\n{}", builder.command_line());
    }
    Ok(())
}

fn save_flow(b: &Builder) -> std::io::Result<()> {
    let name = loop {
        let Ok(name) = input("Recipe name? (e.g. `errors`)", "") else {
            return Ok(());
        };
        let name = name.trim().to_owned();
        if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && !name.is_empty()
        {
            break name;
        }
        println!("{}", style("  use letters, digits, - or _").red());
    };

    let targets = save::targets();
    let labels: Vec<&str> = targets.iter().map(|t| t.label.as_str()).collect();
    let Ok(choice) = select("Save where?", &labels) else {
        return Ok(());
    };
    let target = &targets[choice];

    let block = b.to_recipe_toml(&name);
    let duplicate = save::append_recipe(&target.path, &name, &block)?;
    if duplicate {
        println!(
            "{}",
            style(format!(
                "  note: [recipe.{name}] already existed in that file — appended another; edit to keep one"
            ))
            .yellow()
        );
    }
    println!(
        "{} {}",
        style("✓ saved").green(),
        style(format!("@{name} → {}", target.path.display())).dim()
    );
    println!("{}", style(format!("  run it any time with: jlf @{name}")).dim());
    Ok(())
}
