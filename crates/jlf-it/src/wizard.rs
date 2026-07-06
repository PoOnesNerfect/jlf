//! The interactive builder: pick a mode, then edit any part and see the preview
//! refresh after every change, before running or saving it.

use std::path::PathBuf;

use console::{style, Term};
use dialoguer::{
    theme::{ColorfulTheme, Theme},
    Confirm, Input, Select,
};

use crate::builder::{Builder, Mode};
use crate::{preview, save};

/// Outcome of a prompt: a value, or the user asked to quit (Esc/Ctrl-C).
type Prompt<T> = Result<T, ()>;

fn theme() -> ColorfulTheme {
    ColorfulTheme::default()
}

/// An `Input` theme that puts the typed value on its own line, below the prompt.
struct NewlineInput;

impl Theme for NewlineInput {
    fn format_input_prompt(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        default: Option<&str>,
    ) -> std::fmt::Result {
        if !prompt.is_empty() {
            writeln!(f, "{}", style(prompt).cyan())?;
        }
        match default {
            Some(d) => write!(f, "{} {} ", style(format!("({d})")).dim(), style("›").cyan()),
            None => write!(f, "{} ", style("›").cyan()),
        }
    }
}

fn select(prompt: &str, items: &[String], default: usize) -> Prompt<usize> {
    let default = default.min(items.len().saturating_sub(1));
    let r = Select::with_theme(&theme())
        .with_prompt(prompt)
        .items(items)
        .default(default)
        .interact_opt();
    if r.is_err() {
        show_cursor();
    }
    r.map_err(|_| ())?.ok_or(())
}

/// A `Select` that starts on the current value.
fn select_default(prompt: &str, items: &[&str], default: usize) -> Option<usize> {
    let labels: Vec<String> = items.iter().map(|s| s.to_string()).collect();
    Select::with_theme(&theme())
        .with_prompt(prompt)
        .items(&labels)
        .default(default)
        .interact_opt()
        .ok()
        .flatten()
}

fn input(prompt: &str, initial: &str) -> Prompt<String> {
    let mut b = Input::<String>::with_theme(&NewlineInput)
        .with_prompt(prompt)
        .allow_empty(true);
    if !initial.is_empty() {
        b = b.with_initial_text(initial.to_owned());
    }
    let r = b.interact_text();
    if r.is_err() {
        show_cursor();
    }
    r.map_err(|_| ())
}

fn split_commas(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Does this input read as a `$`-template (vs a plain field list)? The `$` DSL
/// always uses `$`, so any `$` means a template.
fn is_template(s: &str) -> bool {
    s.contains('$')
}

/// The top-level keys of the first sample record, shown as a hint.
fn available_fields(jlf: &std::path::Path, sample: &str) -> String {
    let first = sample.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    preview::run(jlf, &["$( $key )\", \"*".into()], first)
        .trim()
        .to_owned()
}

/// Restore the terminal cursor on both streams (dialoguer hides it on stderr).
/// Safe to call from a Ctrl-C handler.
pub fn show_cursor() {
    for t in [Term::stdout(), Term::stderr()] {
        let _ = t.show_cursor();
        let _ = t.flush();
    }
}

/// Restores the terminal cursor when dropped (dialoguer hides it during prompts).
struct CursorGuard;

impl Drop for CursorGuard {
    fn drop(&mut self) {
        show_cursor();
    }
}

pub fn run(sample: String, jlf: PathBuf) -> std::io::Result<()> {
    // Show the cursor again however this returns (normal, `?`, or panic).
    let _cursor = CursorGuard;
    let term = Term::stdout();
    let fields = available_fields(&jlf, &sample);

    let Some(mut b) = start() else {
        println!("\n{}", style("cancelled").dim());
        return Ok(());
    };

    // Keep the menu highlight where the user last left it, rather than resetting
    // to the top after each edit.
    let mut cursor = 0usize;
    loop {
        render(&term, &b, &jlf, &sample, &fields);

        let items = menu(&b);
        let labels: Vec<String> = items.iter().map(|(label, _)| label.clone()).collect();
        let Ok(choice) = select("Edit a part, or act", &labels, cursor) else {
            return Ok(());
        };
        cursor = choice;
        match items[choice].1 {
            Act::EditFilters => edit_filters(&mut b),
            Act::EditView => edit_view(&mut b),
            Act::ToggleCompact => b.compact = !b.compact,
            Act::EditRedact => edit_redact(&mut b),
            Act::EditSummary => edit_summary(&mut b),
            Act::EditExport => edit_export(&mut b),
            Act::EditOutputFormat => edit_output_format(&mut b),
            Act::ChangeMode => {
                if let Some(nb) = start() {
                    b = nb;
                }
            }
            Act::Run => {
                println!("\n{}", style("── output ───────────────").dim());
                print!("{}", preview::run(&jlf, &b.to_args(), &sample));
                println!("{}", style("─────────────────────────").dim());
                return Ok(());
            }
            Act::Save => {
                save_flow(&b)?;
                let _ = Confirm::with_theme(&theme())
                    .with_prompt("continue")
                    .default(true)
                    .interact();
            }
            Act::Quit => return Ok(()),
        }
    }
}

/// Render the current state: header, the equivalent command, and a live preview.
fn render(term: &Term, b: &Builder, jlf: &std::path::Path, sample: &str, fields: &str) {
    let _ = term.clear_screen();
    println!("{}", style(" jlf it — build a command ").black().on_cyan());
    if !fields.is_empty() {
        println!("{} {}", style("fields:").dim(), style(fields).dim());
    }
    println!("\n{} {}", style("command").dim(), style(b.command_line()).cyan());
    println!("{}", style("── preview ──────────────").dim());
    let out = preview::run(jlf, &b.to_args(), sample);
    for line in out.lines().take(15) {
        println!("  {line}");
    }
    if out.lines().count() > 15 {
        println!("  {}", style("… (truncated)").dim());
    }
    println!("{}\n", style("─────────────────────────").dim());
}

/// Menu actions, paired with their labels (which show the current value).
#[derive(Clone, Copy)]
enum Act {
    EditFilters,
    EditView,
    ToggleCompact,
    EditRedact,
    EditSummary,
    EditExport,
    EditOutputFormat,
    ChangeMode,
    Run,
    Save,
    Quit,
}

fn shown(s: &str) -> String {
    if s.is_empty() {
        style("(none)").dim().to_string()
    } else {
        s.to_owned()
    }
}

fn menu(b: &Builder) -> Vec<(String, Act)> {
    let mut m: Vec<(String, Act)> = vec![(
        format!("Filters — {}", shown(&b.filters.join(" "))),
        Act::EditFilters,
    )];
    match b.mode() {
        Mode::View => {
            let layout = b.template.clone().unwrap_or_else(|| b.fields.join(","));
            m.push((format!("Template / fields — {}", shown(&layout)), Act::EditView));
            m.push((
                format!("Compact — {}", if b.compact { "on" } else { "off" }),
                Act::ToggleCompact,
            ));
            m.push((format!("Redact — {}", shown(&b.redact.join(","))), Act::EditRedact));
        }
        Mode::Summarize => {
            let s = format!(
                "{} {}{}",
                b.verb.as_deref().unwrap_or("count"),
                b.field.as_deref().unwrap_or(""),
                b.by.as_deref().map(|x| format!(" by {x}")).unwrap_or_default(),
            );
            m.push((format!("Summary — {}", s.trim()), Act::EditSummary));
            m.push((
                format!("Output format — {}", shown(b.format.as_deref().unwrap_or(""))),
                Act::EditOutputFormat,
            ));
        }
        Mode::Export => {
            m.push((
                format!(
                    "Table — {} of {}",
                    b.format.as_deref().unwrap_or("csv"),
                    shown(&b.fields.join(","))
                ),
                Act::EditExport,
            ));
        }
    }
    m.push(("Change mode".into(), Act::ChangeMode));
    m.push((style("▶ Run it").green().to_string(), Act::Run));
    m.push(("Save as a recipe".into(), Act::Save));
    m.push(("Quit".into(), Act::Quit));
    m
}

/// Pick the mode, returning a fresh builder seeded for it.
fn start() -> Option<Builder> {
    let mode = match select(
        "What do you want to build?",
        &[
            "View — pretty-print / pick fields".into(),
            "Summarize — count / stats / top / uniq".into(),
            "Export — a csv / tsv / md table".into(),
        ],
        0,
    ) {
        Ok(0) => Mode::View,
        Ok(1) => Mode::Summarize,
        Ok(2) => Mode::Export,
        _ => return None,
    };
    let mut b = Builder::default();
    match mode {
        Mode::View => {}
        Mode::Summarize => b.verb = Some("count".into()),
        Mode::Export => b.format = Some("csv".into()),
    }
    Some(b)
}

fn edit_filters(b: &mut Builder) {
    if let Ok(s) = input(
        "Filters (e.g. `level=error status>=500`, blank to clear)",
        &b.filters.join(" "),
    ) {
        b.filters = s.split_whitespace().map(str::to_owned).collect();
    }
}

fn edit_view(b: &mut Builder) {
    let initial = b.template.clone().unwrap_or_else(|| b.fields.join(","));
    let Ok(s) = input(
        "Template or fields (`$ts $msg` / `${level}` = template, `ts,level` = fields, blank = default)",
        &initial,
    ) else {
        return;
    };
    let s = s.trim();
    if s.is_empty() {
        b.template = None;
        b.fields.clear();
    } else if is_template(s) {
        b.template = Some(s.to_owned());
        b.fields.clear();
    } else {
        b.fields = split_commas(s);
        b.template = None;
    }
}

fn edit_redact(b: &mut Builder) {
    if let Ok(s) = input(
        "Redact fields (comma globs like `token,*.email`, blank to clear)",
        &b.redact.join(","),
    ) {
        b.redact = split_commas(&s);
    }
}

fn edit_summary(b: &mut Builder) {
    const VERBS: [&str; 4] = ["count", "stats", "top", "uniq"];
    let cur = VERBS.iter().position(|v| Some(*v) == b.verb.as_deref()).unwrap_or(0);
    let Some(idx) = select_default("Which summary?", &VERBS, cur) else {
        return;
    };
    b.verb = Some(VERBS[idx].to_owned());

    let prompt = if VERBS[idx] == "count" {
        "Field to break down by? (blank = just the total)"
    } else {
        "Field?"
    };
    if let Ok(f) = input(prompt, b.field.as_deref().unwrap_or("")) {
        b.field = (!f.trim().is_empty()).then(|| f.trim().to_owned());
    }
    if VERBS[idx] == "top" {
        let cur_n = b.n.map(|n| n.to_string()).unwrap_or_default();
        if let Ok(n) = input("How many (top N)?", &cur_n) {
            b.n = n.trim().parse().ok();
        }
    }
    if let Ok(by) = input("Group by a field? (blank to skip)", b.by.as_deref().unwrap_or("")) {
        b.by = (!by.trim().is_empty()).then(|| by.trim().to_owned());
    }
}

fn edit_export(b: &mut Builder) {
    const FMTS: [&str; 3] = ["csv", "tsv", "md"];
    let cur = FMTS.iter().position(|v| Some(*v) == b.format.as_deref()).unwrap_or(0);
    if let Some(idx) = select_default("Table format?", &FMTS, cur) {
        b.format = Some(FMTS[idx].to_owned());
    }
    if let Ok(cols) = input("Columns (comma-separated, e.g. `ts,level,msg`)", &b.fields.join(",")) {
        b.fields = split_commas(&cols);
    }
}

fn edit_output_format(b: &mut Builder) {
    const OPTS: [&str; 4] = ["text", "csv", "tsv", "md"];
    let cur = match b.format.as_deref() {
        Some("csv") => 1,
        Some("tsv") => 2,
        Some("md") => 3,
        _ => 0,
    };
    if let Some(idx) = select_default("Output as?", &OPTS, cur) {
        b.format = (idx != 0).then(|| OPTS[idx].to_owned());
    }
}

fn save_flow(b: &Builder) -> std::io::Result<()> {
    let name = loop {
        let Ok(name) = input("Recipe name? (e.g. `errors`)", "") else {
            return Ok(());
        };
        let name = name.trim().to_owned();
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            break name;
        }
        println!("{}", style("  use letters, digits, - or _").red());
    };

    let targets = save::targets();
    let labels: Vec<String> = targets.iter().map(|t| t.label.clone()).collect();
    let Ok(choice) = select("Save where?", &labels, 0) else {
        return Ok(());
    };
    let target = &targets[choice];

    let block = b.to_recipe_toml(&name);
    let duplicate = save::append_recipe(&target.path, &name, &block)?;
    if duplicate {
        println!(
            "{}",
            style(format!(
                "  note: [recipe.{name}] already existed — appended another; edit to keep one"
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
