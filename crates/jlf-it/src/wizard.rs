//! The interactive builder: pick a mode, then edit any part with a LIVE preview
//! that refreshes on every keystroke, before running or saving it. When a filter
//! matches nothing in the sample, the preview falls back to a synthesized record
//! (see `synth`) so you can still see the shape of the result.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use console::{style, Key, Term};
use dialoguer::{theme::ColorfulTheme, theme::Theme, Confirm, Input, Select};
use serde_json::Value;

use crate::builder::{Builder, Mode};
use crate::fields::{self, Catalog};
use crate::{preview, sample, save, synth};

/// Outcome of a prompt: committed, or the user backed out (Esc/Ctrl-C).
type Prompt = Result<(), ()>;

/// Which completions a field applies while it's being edited.
#[derive(Clone, Copy, PartialEq)]
enum Complete {
    /// No suggestions (free text, numbers).
    None,
    /// A filter expression: complete the field, then operators, then values.
    Filter,
    /// A single field path.
    Field,
    /// A comma-separated list of field paths (columns, redact globs).
    List,
}

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

/// What every render needs: the terminal, the `jlf` binary, and the sample.
struct Ctx<'a> {
    term: &'a Term,
    jlf: &'a Path,
    sample: &'a str,
    cat: &'a Catalog,
    /// The richest sample record, shown (colored) so the user sees the raw data.
    example: Option<&'a Value>,
}

/// A discrete `Select` (mode pick, save target) — no live preview needed.
fn select(prompt: &str, items: &[String], default: usize) -> Result<usize, ()> {
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

/// A plain one-line text prompt (used for the save-recipe name, where there's
/// nothing to preview).
fn input(prompt: &str, initial: &str) -> Result<String, ()> {
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

/// Does this input read as a `$`-template (vs a plain field list)?
fn is_template(s: &str) -> bool {
    s.contains('$')
}

/// Restore the terminal cursor on both streams (dialoguer hides it on stderr).
/// Safe to call from a Ctrl-C handler.
pub fn show_cursor() {
    for t in [Term::stdout(), Term::stderr()] {
        let _ = t.show_cursor();
        let _ = t.flush();
    }
}

/// Restores the terminal cursor when dropped (prompts hide it).
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
    let cat = Catalog::from_sample(&sample, 500);
    let example = sample::richest(&sample);
    let ctx = Ctx {
        term: &term,
        jlf: &jlf,
        sample: &sample,
        cat: &cat,
        example: example.as_ref(),
    };

    let Some(mut b) = start() else {
        println!("\n{}", style("cancelled").dim());
        return Ok(());
    };

    // Keep the menu highlight where the user last left it.
    let mut cursor = 0usize;
    loop {
        draw_body(&ctx, &b, false, &HashSet::new());

        let items = menu(&b);
        let labels: Vec<String> = items.iter().map(|(label, _)| label.clone()).collect();
        let Ok(choice) = select("Edit a part, or act", &labels, cursor) else {
            return Ok(());
        };
        cursor = choice;
        match items[choice].1 {
            Act::EditFilters => {
                let _ = edit_filters(&ctx, &mut b);
            }
            Act::EditView => {
                let _ = edit_view(&ctx, &mut b);
            }
            Act::ToggleCompact => b.compact = !b.compact,
            Act::EditRedact => {
                let _ = edit_redact(&ctx, &mut b);
            }
            Act::EditSummary => edit_summary(&ctx, &mut b),
            Act::EditExport => edit_export(&ctx, &mut b),
            Act::EditOutputFormat => {
                let _ = edit_output_format(&ctx, &mut b);
            }
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

/// Run the preview, falling back to a synthesized matching record when the
/// filters exclude everything in the sample. Returns `(output, synthesized?)`.
fn preview_or_synth(jlf: &Path, b: &Builder, sample: &str) -> (String, bool) {
    let out = preview::run(jlf, &b.to_args(), sample);
    if out.trim() == "(no matching records)" && !b.filters.is_empty() {
        let first = sample.lines().find(|l| !l.trim().is_empty()).unwrap_or("{}");
        if let Some(line) = synth::synthesize(first, &b.filters) {
            let s = preview::run(jlf, &b.to_args(), &line);
            let t = s.trim();
            if !t.is_empty() && t != "(no matching records)" && !t.starts_with("(no output)") {
                return (s, true);
            }
        }
    }
    (out, false)
}

/// A preview that keeps every record on screen: matching ones in color, the rest
/// dimmed. Used while editing filters (View mode) so records don't vanish as you
/// type a not-yet-finished filter.
fn preview_dim(jlf: &Path, b: &Builder, sample: &str) -> String {
    let mut args = vec!["--color=always".to_owned(), "--dim-unmatched".to_owned()];
    args.extend(b.to_args());
    preview::run(jlf, &args, sample)
}

/// Render the header, the equivalent command, the raw sample record (colored,
/// with typed fields highlighted), and the live preview. Shared by the menu and
/// every edit session. Panels are padded to a terminal-height-adaptive height so
/// nothing below them shifts as content changes. `dim` dims non-matching records
/// in the preview; `highlight` is the set of field paths to light up.
fn draw_body(ctx: &Ctx, b: &Builder, dim: bool, highlight: &HashSet<String>) {
    let _ = ctx.term.clear_screen();
    println!("{}", style(" jlf it — build a command ").black().on_cyan());
    println!("\n{} {}", style("command").dim(), style(b.command_line()).cyan());

    // Split the free rows between the sample record and the preview.
    let (rows, _) = ctx.term.size();
    let avail = (rows as usize).saturating_sub(11).max(8);
    let sample_rows = if ctx.example.is_some() {
        (avail * 2 / 5).clamp(3, 12)
    } else {
        0
    };
    let preview_rows = avail.saturating_sub(sample_rows).clamp(3, 16);

    if let Some(ex) = ctx.example {
        println!("{}", style("── sample record · matches highlighted ──").dim());
        emit_rows(&sample::render(ex, highlight), sample_rows);
    }

    let (out, synthesized) = if dim {
        (preview_dim(ctx.jlf, b, ctx.sample), false)
    } else {
        preview_or_synth(ctx.jlf, b, ctx.sample)
    };
    let label = if synthesized {
        style("── preview · synthesized example (nothing matched) ──").yellow()
    } else if dim {
        style("── preview · matches in color, others dimmed ──").dim()
    } else {
        style("── preview ──────────────").dim()
    };
    println!("{label}");
    emit_rows(&out, preview_rows);
    println!("{}", style("─────────────────────────").dim());
}

/// Print exactly `rows` lines of `text` (indented), padding with blanks and
/// collapsing any overflow into a `… (+N more)` line, so the height is fixed.
fn emit_rows(text: &str, rows: usize) {
    let lines: Vec<&str> = text.lines().collect();
    for i in 0..rows {
        if lines.len() > rows && i == rows - 1 {
            let more = lines.len() - (rows - 1);
            println!("  {}", style(format!("… (+{more} more)")).dim());
        } else if i < lines.len() {
            println!("  {}", lines[i]);
        } else {
            println!();
        }
    }
}

/// A text field edited in place: the preview (built from `b` with `apply(buf)`)
/// refreshes on every keystroke. `dim` selects the dimming filter preview;
/// `complete` drives autocompletion of fields, then operators, then values.
///
/// Keys: Tab/Shift-Tab and ↑/↓ move the highlighted suggestion; Enter accepts the
/// highlight (or commits the field when there's nothing to accept); ←/→/Home/End
/// move the cursor; Esc backs out.
fn live_edit(
    ctx: &Ctx,
    b: &mut Builder,
    prompt: &str,
    initial: &str,
    dim: bool,
    complete: Complete,
    apply: impl Fn(&mut Builder, &str),
) -> Prompt {
    let mut buf: Vec<char> = initial.chars().collect();
    let mut pos = buf.len();
    let mut sel = 0usize; // highlighted suggestion
    let mut dismissed = false; // suggestions hidden (Esc) so Enter commits
    loop {
        let s: String = buf.iter().collect();
        let mut preview_b = b.clone();
        apply(&mut preview_b, &s);
        let highlight = fields::active_fields(&s, &ctx.cat.paths);
        draw_body(ctx, &preview_b, dim, &highlight);

        let (tok_start, tok_end, all) = suggestions(complete, &buf, pos, ctx.cat);
        let cands = if dismissed { Vec::new() } else { all };
        sel = if cands.is_empty() { 0 } else { sel.min(cands.len() - 1) };

        // Always emit the suggestion + hint lines (blank when none) so the input
        // row never moves.
        if cands.is_empty() {
            println!();
            println!();
        } else {
            let row = cands
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if i == sel {
                        style(format!(" {c} ")).black().on_cyan().to_string()
                    } else {
                        style(format!(" {c} ")).dim().to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            println!("{} {}", style("⇥").dim(), row);
            println!("  {}", style("Tab/↑↓ pick · ⏎ fill · Esc dismiss").dim());
        }

        println!("{}", style(prompt).cyan());
        print!("{} {}", style("›").cyan(), s);
        let _ = std::io::stdout().flush();
        let trailing = buf.len() - pos;
        if trailing > 0 {
            let _ = ctx.term.move_cursor_left(trailing);
        }
        let _ = ctx.term.show_cursor();

        match ctx.term.read_key() {
            // Enter fills the highlighted suggestion (even from an empty buffer);
            // with no suggestions showing it commits the field.
            Ok(Key::Enter) => {
                if !cands.is_empty() {
                    let repl: Vec<char> = cands[sel].chars().collect();
                    buf.splice(tok_start..tok_end, repl.iter().copied());
                    pos = tok_start + repl.len();
                    sel = 0;
                } else {
                    apply(b, &s);
                    return Ok(());
                }
            }
            // Esc first dismisses the suggestions (so the next Enter commits),
            // then backs out of the edit.
            Ok(Key::Escape) => {
                if !cands.is_empty() {
                    dismissed = true;
                } else {
                    return Err(());
                }
            }
            Ok(Key::Tab) | Ok(Key::ArrowDown) => {
                dismissed = false;
                let n = suggestions(complete, &buf, pos, ctx.cat).2.len();
                if n > 0 {
                    sel = (sel + 1) % n;
                }
            }
            Ok(Key::BackTab) | Ok(Key::ArrowUp) => {
                dismissed = false;
                let n = suggestions(complete, &buf, pos, ctx.cat).2.len();
                if n > 0 {
                    sel = (sel + n - 1) % n;
                }
            }
            Ok(Key::Char(c)) => {
                match c {
                    '\u{17}' => delete_word_back(&mut buf, &mut pos), // Ctrl-W
                    '\u{15}' => {
                        buf.drain(0..pos); // Ctrl-U: to start of line
                        pos = 0;
                    }
                    '\u{0b}' => buf.truncate(pos), // Ctrl-K: to end of line
                    '\u{04}' if pos < buf.len() => {
                        buf.remove(pos); // Ctrl-D: delete forward
                    }
                    c if (c as u32) < 0x20 => {} // ignore other control chars
                    c => {
                        buf.insert(pos, c);
                        pos += 1;
                    }
                }
                sel = 0;
                dismissed = false;
            }
            Ok(Key::Backspace) if pos > 0 => {
                pos -= 1;
                buf.remove(pos);
                sel = 0;
                dismissed = false;
            }
            Ok(Key::Del) if pos < buf.len() => {
                buf.remove(pos);
                sel = 0;
                dismissed = false;
            }
            Ok(Key::ArrowLeft) => pos = pos.saturating_sub(1),
            Ok(Key::ArrowRight) if pos < buf.len() => pos += 1,
            Ok(Key::Home) => pos = 0,
            Ok(Key::End) => pos = buf.len(),
            Ok(_) => {}
            Err(_) => return Err(()),
        }
    }
}

/// Delete the word before the cursor (any whitespace, then non-whitespace).
fn delete_word_back(buf: &mut Vec<char>, pos: &mut usize) {
    let mut i = *pos;
    while i > 0 && buf[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !buf[i - 1].is_whitespace() {
        i -= 1;
    }
    buf.drain(i..*pos);
    *pos = i;
}

/// Compute the token span at the cursor and its ranked completions for `ctx`.
/// Returns `(start, end, candidates)`; Tab replaces `buf[start..end]` with the
/// first candidate.
fn suggestions(ctx: Complete, buf: &[char], pos: usize, cat: &Catalog) -> (usize, usize, Vec<String>) {
    match ctx {
        Complete::None => (pos, pos, Vec::new()),
        Complete::Field => {
            let (start, end) = trimmed_span(buf, 0, buf.len());
            let token: String = buf[start..end].iter().collect();
            (start, end, fields::match_paths(&cat.paths, &token, 8))
        }
        Complete::List => {
            let mut seg_start = pos;
            while seg_start > 0 && buf[seg_start - 1] != ',' {
                seg_start -= 1;
            }
            let mut seg_end = pos;
            while seg_end < buf.len() && buf[seg_end] != ',' {
                seg_end += 1;
            }
            let (start, end) = trimmed_span(buf, seg_start, seg_end);
            let token: String = buf[start..end].iter().collect();
            (start, end, fields::match_paths(&cat.paths, &token, 8))
        }
        Complete::Filter => filter_suggestions(buf, pos, cat),
    }
}

/// Filter context: locate the word at the cursor and complete its field part,
/// then operators once the field is exact, then the field's values after an
/// operator.
fn filter_suggestions(buf: &[char], pos: usize, cat: &Catalog) -> (usize, usize, Vec<String>) {
    let mut ws = pos;
    while ws > 0 && !buf[ws - 1].is_whitespace() {
        ws -= 1;
    }
    let mut we = pos;
    while we < buf.len() && !buf[we].is_whitespace() {
        we += 1;
    }
    let word = &buf[ws..we];
    match find_op(word) {
        Some((op_at, op_len)) => {
            let field: String = word[..op_at].iter().collect();
            let field = field.split('|').next().unwrap_or(&field);
            let val_start = ws + op_at + op_len;
            if pos >= val_start {
                let token: String = buf[val_start..we].iter().collect();
                (val_start, we, fields::match_values(cat.values(field), &token, 8))
            } else {
                let token: String = word[..op_at].iter().collect();
                (ws, ws + op_at, fields::match_paths(&cat.paths, &token, 8))
            }
        }
        None => {
            let token: String = word.iter().collect();
            if !token.is_empty() && cat.paths.iter().any(|p| p == &token) {
                // field fully typed -> offer operators
                (we, we, fields::OPS.iter().map(|s| s.to_string()).collect())
            } else {
                (ws, we, fields::match_paths(&cat.paths, &token, 8))
            }
        }
    }
}

/// Find the first filter operator in a word, returning `(char index, length)`.
fn find_op(word: &[char]) -> Option<(usize, usize)> {
    const OPS2: [[char; 2]; 4] = [['!', '='], ['>', '='], ['<', '='], ['!', '~']];
    const OPS1: [char; 4] = ['~', '=', '>', '<'];
    for i in 0..word.len() {
        for op in OPS2 {
            if word.get(i) == Some(&op[0]) && word.get(i + 1) == Some(&op[1]) {
                return Some((i, 2));
            }
        }
        if OPS1.contains(&word[i]) {
            return Some((i, 1));
        }
    }
    None
}

/// Narrow `[start, end)` to exclude surrounding whitespace.
fn trimmed_span(buf: &[char], start: usize, end: usize) -> (usize, usize) {
    let mut s = start;
    while s < end && buf[s].is_whitespace() {
        s += 1;
    }
    let mut e = end;
    while e > s && buf[e - 1].is_whitespace() {
        e -= 1;
    }
    (s, e)
}

/// A choice edited in place: the preview reflects the highlighted option live.
/// Arrows or number keys move; Enter commits, Esc backs out.
fn live_select(
    ctx: &Ctx,
    b: &mut Builder,
    prompt: &str,
    items: &[&str],
    default: usize,
    apply: impl Fn(&mut Builder, usize),
) -> Prompt {
    let mut sel = default.min(items.len().saturating_sub(1));
    loop {
        let mut preview_b = b.clone();
        apply(&mut preview_b, sel);
        draw_body(ctx, &preview_b, false, &HashSet::new());

        println!("{}", style(prompt).cyan());
        for (i, it) in items.iter().enumerate() {
            if i == sel {
                println!("{} {}", style("❯").cyan(), style(it).cyan().bold());
            } else {
                println!("  {}", style(it).dim());
            }
        }
        let _ = ctx.term.hide_cursor();

        match ctx.term.read_key() {
            Ok(Key::Enter) => {
                let _ = ctx.term.show_cursor();
                apply(b, sel);
                return Ok(());
            }
            Ok(Key::Escape) => {
                let _ = ctx.term.show_cursor();
                return Err(());
            }
            Ok(Key::ArrowUp) => sel = sel.saturating_sub(1),
            Ok(Key::ArrowDown) if sel + 1 < items.len() => sel += 1,
            Ok(Key::Char(c)) if c.is_ascii_digit() => {
                let d = (c as usize) - ('0' as usize);
                if d >= 1 && d <= items.len() {
                    sel = d - 1;
                }
            }
            _ => {}
        }
    }
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
    let mode = match b.mode() {
        Mode::View => "view",
        Mode::Summarize => "summarize",
        Mode::Export => "export",
    };
    m.push((format!("⇄ Change mode — {}", style(mode).cyan()), Act::ChangeMode));
    m.push((style("▶ Run it").green().bold().to_string(), Act::Run));
    m.push((style("✎ Save as a recipe").cyan().to_string(), Act::Save));
    m.push((style("✕ Quit").red().to_string(), Act::Quit));
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

fn edit_filters(ctx: &Ctx, b: &mut Builder) -> Prompt {
    let initial = b.filters.join(" ");
    // Dim (rather than drop) non-matching records while editing a View filter, so
    // the log stays on screen as the half-typed filter matches nothing yet.
    let dim = matches!(b.mode(), Mode::View);
    live_edit(
        ctx,
        b,
        "Filters (e.g. `level=error status>=500`, blank to clear)",
        &initial,
        dim,
        Complete::Filter,
        |b, s| b.filters = s.split_whitespace().map(str::to_owned).collect(),
    )
}

fn edit_view(ctx: &Ctx, b: &mut Builder) -> Prompt {
    let initial = b.template.clone().unwrap_or_else(|| b.fields.join(","));
    live_edit(
        ctx,
        b,
        "Template or fields (`$ts $msg` / `${level}` = template, `ts,level` = fields, blank = default)",
        &initial,
        false,
        Complete::None,
        |b, s| {
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
        },
    )
}

fn edit_redact(ctx: &Ctx, b: &mut Builder) -> Prompt {
    let initial = b.redact.join(",");
    live_edit(
        ctx,
        b,
        "Redact fields (comma globs like `token,*.email`, blank to clear)",
        &initial,
        false,
        Complete::List,
        |b, s| b.redact = split_commas(s),
    )
}

fn edit_summary(ctx: &Ctx, b: &mut Builder) {
    const VERBS: [&str; 4] = ["count", "stats", "top", "uniq"];
    let cur = VERBS.iter().position(|v| Some(*v) == b.verb.as_deref()).unwrap_or(0);
    let _ = live_select(ctx, b, "Which summary?", &VERBS, cur, |b, i| {
        b.verb = Some(VERBS[i].to_owned())
    });

    let verb = b.verb.clone().unwrap_or_default();
    let prompt = if verb == "count" {
        "Field to break down by? (blank = just the total)"
    } else {
        "Field?"
    };
    let field0 = b.field.clone().unwrap_or_default();
    let _ = live_edit(ctx, b, prompt, &field0, false, Complete::Field, |b, s| {
        b.field = (!s.trim().is_empty()).then(|| s.trim().to_owned())
    });

    if verb == "top" {
        let n0 = b.n.map(|n| n.to_string()).unwrap_or_default();
        let _ = live_edit(ctx, b, "How many (top N)?", &n0, false, Complete::None, |b, s| {
            b.n = s.trim().parse().ok()
        });
    }
    let by0 = b.by.clone().unwrap_or_default();
    let _ = live_edit(ctx, b, "Group by a field? (blank to skip)", &by0, false, Complete::Field, |b, s| {
        b.by = (!s.trim().is_empty()).then(|| s.trim().to_owned())
    });
}

fn edit_export(ctx: &Ctx, b: &mut Builder) {
    const FMTS: [&str; 3] = ["csv", "tsv", "md"];
    let cur = FMTS.iter().position(|v| Some(*v) == b.format.as_deref()).unwrap_or(0);
    let _ = live_select(ctx, b, "Table format?", &FMTS, cur, |b, i| {
        b.format = Some(FMTS[i].to_owned())
    });
    let cols0 = b.fields.join(",");
    let _ = live_edit(
        ctx,
        b,
        "Columns (comma-separated, e.g. `ts,level,msg`)",
        &cols0,
        false,
        Complete::List,
        |b, s| b.fields = split_commas(s),
    );
}

fn edit_output_format(ctx: &Ctx, b: &mut Builder) -> Prompt {
    const OPTS: [&str; 4] = ["text", "csv", "tsv", "md"];
    let cur = match b.format.as_deref() {
        Some("csv") => 1,
        Some("tsv") => 2,
        Some("md") => 3,
        _ => 0,
    };
    live_select(ctx, b, "Output as?", &OPTS, cur, |b, i| {
        b.format = (i != 0).then(|| OPTS[i].to_owned())
    })
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
