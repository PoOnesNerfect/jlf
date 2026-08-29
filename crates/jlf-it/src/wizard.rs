//! The interactive builder: pick a mode, then edit any part with a LIVE preview
//! that refreshes on every keystroke, before running or saving it. When a
//! filter matches nothing in the sample, the preview falls back to a
//! synthesized record (see `synth`) so you can still see the shape of the
//! result.

use std::{
    collections::HashSet,
    io::Write,
    path::{Path, PathBuf},
};

use console::{style, Key, Term};
use dialoguer::{
    theme::{ColorfulTheme, Theme},
    Confirm, Input, Select,
};
use serde_json::Value;

use crate::{
    builder::{Builder, Mode},
    fields::{self, Catalog},
    input, preview, sample, save, synth,
};

/// Outcome of a prompt: committed, or the user backed out (Esc/Ctrl-C).
type Prompt = Result<(), ()>;

/// Where "Run it" should read from: re-read a file, stream a preserved live
/// pipe, or (fallback) feed the captured sample text.
pub enum RunInput {
    File(PathBuf),
    Live(input::Live),
    Sample,
}

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

fn theme() -> ColorfulTheme { ColorfulTheme::default() }

/// An `Input` theme that puts the typed value on its own line, below the
/// prompt.
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
            Some(d) => write!(
                f,
                "{} {} ",
                style(format!("({d})")).dim(),
                style("›").cyan()
            ),
            None => write!(f, "{} ", style("›").cyan()),
        }
    }
}

/// What every render needs: the terminal, the `jlf` binary, and the sample.
struct Ctx<'a> {
    term: &'a Term,
    jlf: &'a Path,
    sample: &'a str,
    /// A deduped, interest-ordered view of `sample` for record previews (see
    /// [`sample::curate`]). Summaries use the full `sample` instead.
    preview_sample: &'a str,
    cat: &'a Catalog,
    /// The richest sample record, shown (colored) so the user sees the raw
    /// data.
    example: Option<&'a Value>,
}

/// A discrete `Select` (mode pick, save target) — no live preview needed.
/// Runs a dialoguer menu. Returns `Ok(Some(i))` for a pick, `Ok(None)` when the
/// user presses Esc (a soft "back"), and `Err(())` on Ctrl-C (a hard quit).
fn select(
    prompt: &str,
    items: &[String],
    default: usize,
) -> Result<Option<usize>, ()> {
    let default = default.min(items.len().saturating_sub(1));
    let r = Select::with_theme(&theme())
        .with_prompt(prompt)
        .items(items)
        .default(default)
        .interact_opt();
    if r.is_err() {
        show_cursor();
    }
    r.map_err(|_| ())
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

/// Split a filter input line into individual filters. A token that parses as a
/// `field op value` filter starts a new one; tokens that don't are appended to
/// the previous filter's value — so `fields.message~Build mode: DEBUG` stays a
/// single `~` (contains) filter whose value has spaces, instead of being broken
/// into three whitespace tokens.
fn parse_filter_line(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in s.split_whitespace() {
        if out.is_empty() || jlf_core::Filter::parse(tok).is_some() {
            out.push(tok.to_owned());
        } else if let Some(last) = out.last_mut() {
            last.push(' ');
            last.push_str(tok);
        }
    }
    out
}

/// Does this input read as a `$`-template (vs a plain field list)?
fn is_template(s: &str) -> bool { s.contains('$') }

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
    fn drop(&mut self) { show_cursor(); }
}

pub fn run(
    sample: String,
    jlf: PathBuf,
    mut run_input: RunInput,
) -> std::io::Result<()> {
    // Show the cursor again however this returns (normal, `?`, or panic).
    let _cursor = CursorGuard;
    let term = Term::stdout();
    let cat = Catalog::from_sample(&sample, 500);
    let example = sample::richest(&sample);
    // A varied, deduped ordering for record previews; the catalog and richest
    // record still see the full sample so autocomplete stays comprehensive.
    let preview_sample = sample::curate(&sample, 80);
    let ctx = Ctx {
        term: &term,
        jlf: &jlf,
        sample: &sample,
        preview_sample: &preview_sample,
        cat: &cat,
        example: example.as_ref(),
    };

    let Some(mut b) = start() else {
        println!("\n{}", style("cancelled").dim());
        return Ok(());
    };
    // Clear once for a clean canvas; from here every screen repaints in place.
    let _ = term.clear_screen();

    // Keep the menu highlight where the user last left it.
    let mut cursor = 0usize;
    loop {
        let items = menu(&b);
        let choice = match menu_pick(&ctx, &b, &items, cursor) {
            Pick::Item(c) => c,
            // Esc backs out to the mode picker (like "Change mode"); use the
            // "Quit" item or Ctrl-C to leave the builder entirely.
            Pick::Back => match start() {
                Some(nb) => {
                    b = nb;
                    cursor = 0;
                    continue;
                }
                None => return Ok(()),
            },
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
                // start()/save_flow use plain dialoguer prompts; clear the menu
                // first so they don't print on top of it.
                let _ = ctx.term.clear_screen();
                if let Some(nb) = start() {
                    b = nb;
                }
            }
            Act::Run => {
                // Hand the built command the real input (a re-read file or the
                // live pipe), so `docker logs -f | jlf it` keeps streaming
                // instead of stopping at the finite sample.
                let src = std::mem::replace(&mut run_input, RunInput::Sample);
                return run_final(&jlf, &b, &sample, src);
            }
            Act::Save => {
                let _ = ctx.term.clear_screen();
                println!(
                    "{}",
                    style(" jlf it — save a recipe ").black().on_cyan()
                );
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
/// Color is forced on so the framed preview shows level colors etc.; without it
/// `jlf` sees a pipe and disables color.
fn preview_or_synth(jlf: &Path, b: &Builder, sample: &str) -> (String, bool) {
    let args = colored_args(b);
    let out = preview::run(jlf, &args, sample);
    if out.trim() == "(no matching records)" && !b.filters.is_empty() {
        let first = sample
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("{}");
        if let Some(line) = synth::synthesize(first, &b.filters) {
            let s = preview::run(jlf, &args, &line);
            let t = s.trim();
            if !t.is_empty()
                && t != "(no matching records)"
                && !t.starts_with("(no output)")
            {
                return (s, true);
            }
        }
    }
    (out, false)
}

/// Run the built command for real, streaming its output to the terminal. Reads
/// from the original source — re-reading a file in full, or resuming the live
/// pipe (so a `… -f` stream keeps flowing) — falling back to the sample text.
/// jlf inherits the terminal, so it colors and flushes per record (live tail).
fn run_final(
    jlf: &Path,
    b: &Builder,
    sample: &str,
    input: RunInput,
) -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    println!(
        "\n{}",
        style(format!(
            "── running: {} · Ctrl-C to stop ──",
            b.command_line()
        ))
        .dim()
    );

    let mut cmd = Command::new(jlf);
    cmd.args(b.to_args())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    match input {
        RunInput::File(path) => {
            cmd.stdin(Stdio::from(std::fs::File::open(path)?));
            cmd.status()?;
        }
        RunInput::Live(live) => {
            // Feed the sampler's buffered remainder, then splice the live pipe.
            cmd.stdin(Stdio::piped());
            let mut child = cmd.spawn()?;
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(&live.leftover);
                let mut pipe = live.pipe;
                let _ = std::io::copy(&mut pipe, &mut si);
            }
            child.wait()?;
        }
        RunInput::Sample => {
            cmd.stdin(Stdio::piped());
            let mut child = cmd.spawn()?;
            if let Some(mut si) = child.stdin.take() {
                let _ = si.write_all(sample.as_bytes());
            }
            child.wait()?;
        }
    }
    Ok(())
}

/// `jlf` args with color forced on (previews render through a pipe, where jlf
/// would otherwise auto-disable color).
fn colored_args(b: &Builder) -> Vec<String> {
    let mut args = vec!["--color=always".to_owned()];
    args.extend(b.to_args());
    args
}

/// Keep only the records matching `filter_strs`, then curate them (dedup +
/// interest order) so a filtered preview shows varied matches rather than the
/// first repetitive ones. Filtering *before* curation guarantees no matching
/// record is dropped by the dedup step. Unparseable filters are ignored (the
/// preview falls back to curating everything).
fn curate_matching(sample: &str, filter_strs: &[String]) -> String {
    let filters: Vec<jlf_core::Filter> = filter_strs
        .iter()
        .filter_map(|s| jlf_core::Filter::parse(s))
        .collect();
    if filters.is_empty() {
        return sample::curate(sample, 80);
    }
    let mut matching = String::new();
    for line in sample.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut j = jlf_core::Json::Null;
        let ok = j
            .parse_replace(line)
            .map(|()| jlf_core::matches_all(&filters, &j))
            .unwrap_or(false);
        if ok {
            matching.push_str(line);
            matching.push('\n');
        }
    }
    sample::curate(&matching, 80)
}

/// A preview that keeps every record on screen: matching ones in color, the
/// rest dimmed. Used while editing filters (View mode) so records don't vanish
/// as you type a not-yet-finished filter.
fn preview_dim(jlf: &Path, b: &Builder, sample: &str) -> String {
    let mut args = colored_args(b);
    args.insert(1, "--dim-unmatched".to_owned());
    preview::run(jlf, &args, sample)
}

/// Render the title, the raw sample record (colored, with typed fields
/// highlighted), the live preview, and the equivalent command — each in its own
/// bordered frame, with the command frame just above the footer so it's easy to
/// find. Heights adapt to the terminal so the input never shifts; `footer_rows`
/// is how many rows the caller draws below (menu list, or edit prompt).
/// `sample_scroll` scrolls the sample frame.
fn draw_body(
    ctx: &Ctx,
    b: &Builder,
    dim: bool,
    highlight: &HashSet<String>,
    sample_scroll: usize,
    footer_rows: usize,
) {
    let (rows, cols) = ctx.term.size();
    let width = (cols as usize).max(20);
    // Budget: title (1) + footer + the command frame (3) + two content frames
    // (2 border rows each). Whatever's left is split between sample and
    // preview.
    let content = (rows as usize)
        .saturating_sub(1 + 3 + footer_rows + 4)
        .max(6);
    // Summaries show a short aggregate, so give the data (sample) more room.
    let sample_share = if b.mode() == Mode::Summarize { 3 } else { 2 };
    let sample_h = if ctx.example.is_some() {
        (content * sample_share / 5).clamp(3, 30)
    } else {
        0
    };
    let preview_h = content.saturating_sub(sample_h).max(3);

    // Repaint in place from the top-left. Clearing the screen first blanks it
    // for a frame, which reads as a flash while typing; instead we move home
    // and every line erases its own tail (\x1b[K), so the new frame
    // overwrites the old one directly with nothing ever going blank.
    let mut screen = String::from("\x1b[H");
    line(
        &mut screen,
        &style(" jlf it — build a command ")
            .black()
            .on_cyan()
            .to_string(),
    );

    if let Some(ex) = ctx.example {
        let lines: Vec<String> = sample::render(ex, highlight)
            .lines()
            .map(String::from)
            .collect();
        draw_frame(
            &mut screen,
            "sample record · matches highlighted",
            &lines,
            sample_h,
            width,
            sample_scroll,
        );
    }

    // Pick the preview source:
    // - Summaries aggregate over the whole sample (counts must be accurate).
    // - Dim (filter editing) keeps the full sample so matches surface in
    //   context.
    // - A record preview with an active filter curates the *matching* records
    //   (filter first, then dedup) so it shows varied matches, not repeats.
    // - Otherwise, the pre-curated whole-sample view.
    let filtered;
    let src: &str = if b.mode() == Mode::Summarize || dim {
        ctx.sample
    } else if b.filters.is_empty() {
        ctx.preview_sample
    } else {
        filtered = curate_matching(ctx.sample, &b.filters);
        &filtered
    };
    let (pv, synthesized) = if dim {
        (preview_dim(ctx.jlf, b, src), false)
    } else {
        preview_or_synth(ctx.jlf, b, src)
    };
    let title = if synthesized {
        "preview · synthesized example (nothing in the sample matched)"
    } else if dim {
        "preview · matches first (colored), the rest dimmed below"
    } else if b.mode() == Mode::Summarize {
        "preview"
    } else {
        "preview · varied sample (repeats collapsed, errors first)"
    };
    let plines: Vec<String> = pv.lines().map(String::from).collect();
    draw_frame(&mut screen, title, &plines, preview_h, width, 0);

    // The command it builds, framed and near the footer so it's not lost at the
    // top.
    draw_frame(
        &mut screen,
        "command",
        &[style(b.command_line()).cyan().to_string()],
        1,
        width,
        0,
    );

    print!("{screen}");
    let _ = std::io::stdout().flush();
}

/// Append one screen line to `out`: the content, then erase-to-end-of-line so a
/// shorter line cleanly overwrites a longer previous one, then a newline.
fn line(out: &mut String, content: &str) {
    out.push_str(content);
    out.push_str("\x1b[K\n");
}

/// Append a bordered frame `width` wide with `title`, showing `height` rows of
/// `lines` starting at `scroll`. Content is ANSI-aware truncated/padded to the
/// frame width; the bottom border shows how many lines remain below.
fn draw_frame(
    out: &mut String,
    title: &str,
    lines: &[String],
    height: usize,
    width: usize,
    scroll: usize,
) {
    let inner = width - 2; // columns between the border characters
    let cw = inner.saturating_sub(2).max(1); // content width (one space each side)
    let scroll = scroll.min(lines.len().saturating_sub(1));

    // top border: ┌─ title ──────┐
    let seg = format!(" {title} ");
    let segw = console::measure_text_width(&seg);
    line(
        out,
        &format!(
            "{}{}{}{}",
            style("┌─").dim(),
            style(seg).cyan(),
            style("─".repeat(inner.saturating_sub(1 + segw))).dim(),
            style("┐").dim(),
        ),
    );

    for i in 0..height {
        let raw = lines.get(scroll + i).map(String::as_str).unwrap_or("");
        let shown = console::truncate_str(raw, cw, "…");
        let pad = cw.saturating_sub(console::measure_text_width(&shown));
        line(
            out,
            &format!(
                "{} {}{}\u{1b}[0m {}",
                style("│").dim(),
                shown,
                " ".repeat(pad),
                style("│").dim(),
            ),
        );
    }

    // bottom border, indicating scroll position.
    let below = lines.len().saturating_sub(scroll + height);
    let label = if below > 0 {
        format!("─ ↓ {below} more · PgDn ")
    } else if scroll > 0 {
        "─ ↑ PgUp for top ".to_string()
    } else {
        String::new()
    };
    let lw = console::measure_text_width(&label);
    line(
        out,
        &style(format!("└{label}{}┘", "─".repeat(inner.saturating_sub(lw))))
            .dim()
            .to_string(),
    );
}

/// A text field edited in place: the preview (built from `b` with `apply(buf)`)
/// refreshes on every keystroke. `dim` selects the dimming filter preview;
/// `complete` drives autocompletion of fields, then operators, then values.
///
/// Suggestions never auto-select: they show dimmed until you press
/// Tab/Shift-Tab (or ↑/↓), which selects and *fills* successive candidates into
/// the input so Enter applies immediately. Editing the buffer deselects; Enter
/// always commits what's shown; Esc exits without applying; ←/→/Home/End move
/// the cursor.
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
    // Active Tab-cycle over the frozen candidate list, or None when nothing is
    // selected (the initial state, and after any edit).
    let mut cycle: Option<Cycle> = None;
    let mut sample_scroll = 0usize; // PgUp/PgDn scroll of the sample frame
    let sample_len = ctx
        .example
        .map(|e| sample::render(e, &HashSet::new()).lines().count())
        .unwrap_or(0);
    loop {
        let s: String = buf.iter().collect();
        let mut preview_b = b.clone();
        apply(&mut preview_b, &s);
        let highlight = fields::active_fields(&s, &ctx.cat.paths);
        // Footer below the frames: suggestion row + hint row + prompt + input.
        let _ = ctx.term.hide_cursor();
        draw_body(ctx, &preview_b, dim, &highlight, sample_scroll, 4);

        // While cycling, keep showing the frozen candidate list with the active
        // one highlighted; otherwise recompute live suggestions and highlight
        // nothing (Tab selects the first).
        let live = if cycle.is_none() {
            suggestions(complete, &buf, pos, ctx.cat).2
        } else {
            Vec::new()
        };
        let (cands, hi): (&[String], Option<usize>) = match &cycle {
            Some(c) => (&c.cands, Some(c.idx)),
            None => (&live, None),
        };

        // Always emit the suggestion + hint lines (blank when none) so the
        // input row never moves.
        let mut foot = String::new();
        if cands.is_empty() {
            line(&mut foot, "");
            line(&mut foot, "");
        } else {
            let row = cands
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if Some(i) == hi {
                        style(format!(" {c} ")).black().on_cyan().to_string()
                    } else {
                        style(format!(" {c} ")).dim().to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            line(&mut foot, &format!("{} {}", style("⇥").dim(), row));
            line(
                &mut foot,
                &format!(
                    "  {}",
                    style("Tab/↑↓ cycle · ⏎ apply · Esc cancel").dim()
                ),
            );
        }
        line(&mut foot, &style(prompt).cyan().to_string());
        // Input line: erase its tail and everything below it (stale rows from a
        // taller previous frame) so nothing ghosts as the buffer shrinks.
        foot.push_str(&format!("{} {}\x1b[K\x1b[J", style("›").cyan(), s));
        print!("{foot}");

        let trailing = buf.len() - pos;
        if trailing > 0 {
            let _ = ctx.term.move_cursor_left(trailing);
        }
        let _ = ctx.term.show_cursor();
        let _ = std::io::stdout().flush();

        match ctx.term.read_key() {
            // Enter always commits what's shown — Tab has already filled any
            // chosen suggestion, so there's nothing left to accept here. An
            // empty buffer commits the empty value (e.g. clears a
            // filter).
            Ok(Key::Enter) => {
                apply(b, &s);
                return Ok(());
            }
            // Esc exits the field without applying.
            Ok(Key::Escape) => return Err(()),
            // PgDn/PgUp scroll the sample-record frame.
            Ok(Key::PageDown) => {
                sample_scroll =
                    (sample_scroll + 3).min(sample_len.saturating_sub(1));
            }
            Ok(Key::PageUp) => sample_scroll = sample_scroll.saturating_sub(3),
            // Tab/↓ select and fill the next candidate (Shift-Tab/↑ the
            // previous), cycling the frozen list; the first press
            // selects the first item.
            Ok(Key::Tab) | Ok(Key::ArrowDown) => {
                cycle_step(
                    &mut cycle, &mut buf, &mut pos, complete, ctx.cat, 1,
                );
            }
            Ok(Key::BackTab) | Ok(Key::ArrowUp) => {
                cycle_step(
                    &mut cycle, &mut buf, &mut pos, complete, ctx.cat, -1,
                );
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
                cycle = None;
            }
            Ok(Key::Backspace) if pos > 0 => {
                pos -= 1;
                buf.remove(pos);
                cycle = None;
            }
            Ok(Key::Del) if pos < buf.len() => {
                buf.remove(pos);
                cycle = None;
            }
            Ok(Key::ArrowLeft) => {
                pos = pos.saturating_sub(1);
                cycle = None;
            }
            Ok(Key::ArrowRight) if pos < buf.len() => {
                pos += 1;
                cycle = None;
            }
            Ok(Key::Home) => {
                pos = 0;
                cycle = None;
            }
            Ok(Key::End) => {
                pos = buf.len();
                cycle = None;
            }
            Ok(_) => {}
            Err(_) => return Err(()),
        }
    }
}

/// A frozen Tab-cycle over a suggestion list. `cands` is captured when cycling
/// begins so filling successive items doesn't collapse the list; `base` is the
/// text the user had typed (restored when the cycle steps past either end and
/// deselects); `filled` tracks how many chars the current candidate occupies at
/// `start`, so the next step replaces exactly that span.
struct Cycle {
    start: usize,
    base: Vec<char>,
    cands: Vec<String>,
    idx: usize,
    filled: usize,
}

/// Advance the Tab-cycle by `delta` (±1), filling the selected candidate into
/// `buf`. The ring is `[typed text] → 0 → 1 → … → N-1 → [typed text]`, so
/// stepping past either end deselects and restores what you typed. Starting a
/// cycle captures the current token's candidates; a no-op when there are none.
fn cycle_step(
    cycle: &mut Option<Cycle>,
    buf: &mut Vec<char>,
    pos: &mut usize,
    complete: Complete,
    cat: &Catalog,
    delta: isize,
) {
    match cycle.take() {
        None => {
            let (start, end, cands) = suggestions(complete, buf, *pos, cat);
            if cands.is_empty() {
                return;
            }
            let base: Vec<char> = buf[start..end].to_vec();
            let idx = if delta >= 0 { 0 } else { cands.len() - 1 };
            let repl: Vec<char> = cands[idx].chars().collect();
            let filled = repl.len();
            buf.splice(start..end, repl);
            *pos = start + filled;
            *cycle = Some(Cycle {
                start,
                base,
                cands,
                idx,
                filled,
            });
        }
        Some(mut c) => {
            let next = c.idx as isize + delta;
            if next < 0 || next >= c.cands.len() as isize {
                // Stepped past a boundary: deselect and restore the typed text,
                // leaving the cycle ended (taken above).
                let base = std::mem::take(&mut c.base);
                *pos = c.start + base.len();
                buf.splice(c.start..c.start + c.filled, base);
            } else {
                c.idx = next as usize;
                let repl: Vec<char> = c.cands[c.idx].chars().collect();
                let filled = repl.len();
                buf.splice(c.start..c.start + c.filled, repl);
                c.filled = filled;
                *pos = c.start + filled;
                *cycle = Some(c);
            }
        }
    }
}

/// A "word" for line editing is a run of identifier chars (`alnum`/`_`); every
/// other character — whitespace, `,`, `.`, filter operators — is a boundary.
/// This lets Ctrl-W stop at `,`/`.` inside field paths and column lists instead
/// of wiping the whole line when nothing is space-separated.
fn is_word_char(c: char) -> bool { c.is_alphanumeric() || c == '_' }

/// Delete the word before the cursor: skip the run of boundary chars, then the
/// preceding run of word chars (so `a.b.c` deletes `c`, then `.b`, then `.a`).
fn delete_word_back(buf: &mut Vec<char>, pos: &mut usize) {
    let mut i = *pos;
    while i > 0 && !is_word_char(buf[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word_char(buf[i - 1]) {
        i -= 1;
    }
    buf.drain(i..*pos);
    *pos = i;
}

/// Compute the token span at the cursor and its ranked completions for `ctx`.
/// Returns `(start, end, candidates)`; Tab replaces `buf[start..end]` with the
/// first candidate.
fn suggestions(
    ctx: Complete,
    buf: &[char],
    pos: usize,
    cat: &Catalog,
) -> (usize, usize, Vec<String>) {
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
fn filter_suggestions(
    buf: &[char],
    pos: usize,
    cat: &Catalog,
) -> (usize, usize, Vec<String>) {
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
                (
                    val_start,
                    we,
                    fields::match_values(cat.values(field), &token, 8),
                )
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
    const OPS2: [[char; 2]; 4] =
        [['!', '='], ['>', '='], ['<', '='], ['!', '~']];
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
/// Tab/Shift-Tab, ↑/↓, j/k, or a number key move; Enter commits, Esc backs out.
fn live_select(
    ctx: &Ctx,
    b: &mut Builder,
    prompt: &str,
    items: &[&str],
    default: usize,
    apply: impl Fn(&mut Builder, usize),
) -> Prompt {
    let mut sel = default.min(items.len().saturating_sub(1));
    let n = items.len();
    loop {
        let mut preview_b = b.clone();
        apply(&mut preview_b, sel);
        // Footer: the prompt line plus one row per choice.
        let _ = ctx.term.hide_cursor();
        draw_body(ctx, &preview_b, false, &HashSet::new(), 0, items.len() + 2);

        let mut foot = String::new();
        line(&mut foot, &style(prompt).cyan().to_string());
        for (i, it) in items.iter().enumerate() {
            if i == sel {
                line(
                    &mut foot,
                    &format!(
                        "{} {}",
                        style("❯").cyan(),
                        style(it).cyan().bold()
                    ),
                );
            } else {
                line(&mut foot, &format!("  {}", style(it).dim()));
            }
        }
        // Last footer line + erase anything below (stale rows) to avoid ghosts.
        foot.push_str(&format!(
            "{}\x1b[K\x1b[J",
            style("Tab/↑↓/jk move · ⏎ select · Esc back").dim()
        ));
        print!("{foot}");
        let _ = std::io::stdout().flush();

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
            Ok(Key::ArrowDown | Key::Tab | Key::Char('j')) if n > 0 => {
                sel = (sel + 1) % n
            }
            Ok(Key::ArrowUp | Key::BackTab | Key::Char('k')) if n > 0 => {
                sel = (sel + n - 1) % n
            }
            Ok(Key::Char(c)) if c.is_ascii_digit() => {
                let d = (c as usize) - ('0' as usize);
                if d >= 1 && d <= n {
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
            let layout =
                b.template.clone().unwrap_or_else(|| b.fields.join(","));
            let desc = if layout.is_empty() {
                "whole record".to_string()
            } else {
                layout
            };
            m.push((format!("Fields to show — {desc}"), Act::EditView));
            m.push((
                format!("Compact — {}", if b.compact { "on" } else { "off" }),
                Act::ToggleCompact,
            ));
            m.push((
                format!("Redact — {}", shown(&b.redact.join(","))),
                Act::EditRedact,
            ));
        }
        Mode::Summarize => {
            let s = format!(
                "{} {}{}",
                b.verb.as_deref().unwrap_or("count"),
                b.field.as_deref().unwrap_or(""),
                b.by.as_deref()
                    .map(|x| format!(" by {x}"))
                    .unwrap_or_default(),
            );
            m.push((format!("Summary — {}", s.trim()), Act::EditSummary));
            m.push((
                format!(
                    "Output format — {}",
                    shown(b.format.as_deref().unwrap_or(""))
                ),
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
    m.push((
        format!("⇄ Change mode — {}", style(mode).cyan()),
        Act::ChangeMode,
    ));
    m.push((style("▶ Run it").green().bold().to_string(), Act::Run));
    m.push((style("✎ Save as a recipe").cyan().to_string(), Act::Save));
    m.push((style("✕ Quit").red().to_string(), Act::Quit));
    m
}

/// The one-key accelerator for a menu action. Unique within any single mode's
/// menu (Redact uses `d` and Summary uses `y` so Run/Save can keep `r`/`s`).
fn accel(act: Act) -> char {
    match act {
        Act::EditFilters => 'f',
        Act::EditView => 't', // Template / fields (View only)
        Act::ToggleCompact => 'c',
        Act::EditRedact => 'd',
        Act::EditSummary => 'y',
        Act::EditOutputFormat => 'o',
        // Table export never coexists with template editing, so both can use
        // `t`.
        Act::EditExport => 't',
        Act::ChangeMode => 'm',
        Act::Run => 'r',
        Act::Save => 's',
        Act::Quit => 'q',
    }
}

/// Outcome of the main menu: an item was chosen, or the user backed out (Esc).
/// Ctrl-C exits via the global handler, so it needs no variant here.
enum Pick {
    Item(usize),
    Back,
}

/// The main menu: draws the frames plus the action list, each tagged with its
/// one-key accelerator. Enter picks the highlight; pressing an accelerator (or
/// a 1-based digit) jumps straight to that item; ↑↓/jk/Tab move; Esc backs out.
fn menu_pick(
    ctx: &Ctx,
    b: &Builder,
    items: &[(String, Act)],
    start: usize,
) -> Pick {
    let accels: Vec<char> = items.iter().map(|(_, a)| accel(*a)).collect();
    let n = items.len();
    let mut sel = start.min(n.saturating_sub(1));
    loop {
        let _ = ctx.term.hide_cursor();
        draw_body(ctx, b, false, &HashSet::new(), 0, n + 2);

        let mut foot = String::new();
        line(&mut foot, &style("Edit a part, or act").cyan().to_string());
        for (i, ((label, _), key)) in items.iter().zip(&accels).enumerate() {
            let tag = format!("[{key}]");
            if i == sel {
                line(
                    &mut foot,
                    &format!(
                        "{} {} {}",
                        style("❯").cyan(),
                        style(tag).cyan().bold(),
                        style(label).cyan().bold()
                    ),
                );
            } else {
                line(
                    &mut foot,
                    &format!(
                        "  {} {}",
                        style(tag).yellow(),
                        style(label).dim()
                    ),
                );
            }
        }
        foot.push_str(&format!(
            "{}\x1b[K\x1b[J",
            style("letter jumps · ↑↓/jk move · ⏎ select · Esc back").dim()
        ));
        print!("{foot}");
        let _ = std::io::stdout().flush();

        match ctx.term.read_key() {
            Ok(Key::Enter) => {
                let _ = ctx.term.show_cursor();
                return Pick::Item(sel);
            }
            Ok(Key::Escape) => {
                let _ = ctx.term.show_cursor();
                return Pick::Back;
            }
            Ok(Key::ArrowDown | Key::Tab | Key::Char('j')) if n > 0 => {
                sel = (sel + 1) % n
            }
            Ok(Key::ArrowUp | Key::BackTab | Key::Char('k')) if n > 0 => {
                sel = (sel + n - 1) % n
            }
            // A 1-based digit jumps straight to that item.
            Ok(Key::Char(c)) if c.is_ascii_digit() => {
                let d = (c as usize).wrapping_sub('1' as usize);
                if d < n {
                    let _ = ctx.term.show_cursor();
                    return Pick::Item(d);
                }
            }
            // Any other letter that matches an accelerator activates it.
            Ok(Key::Char(c)) => {
                let lc = c.to_ascii_lowercase();
                if let Some(i) = accels.iter().position(|&k| k == lc) {
                    let _ = ctx.term.show_cursor();
                    return Pick::Item(i);
                }
            }
            _ => {}
        }
    }
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
        Ok(Some(0)) => Mode::View,
        Ok(Some(1)) => Mode::Summarize,
        Ok(Some(2)) => Mode::Export,
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
    // Dim (rather than drop) non-matching records while editing a View filter,
    // so the log stays on screen as the half-typed filter matches nothing
    // yet.
    let dim = matches!(b.mode(), Mode::View);
    live_edit(
        ctx,
        b,
        "Filters (e.g. `level=error status>=500`, blank to clear)",
        &initial,
        dim,
        Complete::Filter,
        |b, s| b.filters = parse_filter_line(s),
    )
}

fn edit_view(ctx: &Ctx, b: &mut Builder) -> Prompt {
    let initial = b.template.clone().unwrap_or_else(|| b.fields.join(","));
    live_edit(
        ctx,
        b,
        "Which fields to show on each line? A comma list like  \
         timestamp,level,message  — or blank to show the whole record. \
         (Advanced: a $template like  $level $message.)",
        &initial,
        false,
        Complete::List,
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
        "Redact fields — names or paths like  token, fields.message, *.email  \
         (blank to clear)",
        &initial,
        false,
        Complete::List,
        |b, s| b.redact = split_commas(s),
    )
}

fn edit_summary(ctx: &Ctx, b: &mut Builder) {
    const VERBS: [&str; 4] = ["count", "stats", "top", "uniq"];
    let cur = VERBS
        .iter()
        .position(|v| Some(*v) == b.verb.as_deref())
        .unwrap_or(0);
    // Esc on any step cancels the whole flow (back to the menu), rather than
    // advancing to the next step.
    if live_select(ctx, b, "Which summary?", &VERBS, cur, |b, i| {
        b.verb = Some(VERBS[i].to_owned())
    })
    .is_err()
    {
        return;
    }

    let verb = b.verb.clone().unwrap_or_default();
    let prompt = if verb == "count" {
        "Field to break down by? (blank = just the total)"
    } else {
        "Field?"
    };
    let field0 = b.field.clone().unwrap_or_default();
    if live_edit(ctx, b, prompt, &field0, false, Complete::Field, |b, s| {
        b.field = (!s.trim().is_empty()).then(|| s.trim().to_owned())
    })
    .is_err()
    {
        return;
    }

    if verb == "top" {
        let n0 = b.n.map(|n| n.to_string()).unwrap_or_default();
        if live_edit(
            ctx,
            b,
            "How many (top N)?",
            &n0,
            false,
            Complete::None,
            |b, s| b.n = s.trim().parse().ok(),
        )
        .is_err()
        {
            return;
        }
    }
    let by0 = b.by.clone().unwrap_or_default();
    let _ = live_edit(
        ctx,
        b,
        "Group by a field? (blank to skip)",
        &by0,
        false,
        Complete::Field,
        |b, s| b.by = (!s.trim().is_empty()).then(|| s.trim().to_owned()),
    );
}

fn edit_export(ctx: &Ctx, b: &mut Builder) {
    const FMTS: [&str; 3] = ["csv", "tsv", "md"];
    let cur = FMTS
        .iter()
        .position(|v| Some(*v) == b.format.as_deref())
        .unwrap_or(0);
    // Esc on the format picker cancels the whole flow instead of advancing to
    // the columns editor.
    if live_select(ctx, b, "Table format?", &FMTS, cur, |b, i| {
        b.format = Some(FMTS[i].to_owned())
    })
    .is_err()
    {
        return;
    }
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
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            break name;
        }
        println!("{}", style("  use letters, digits, - or _").red());
    };

    let targets = save::targets();
    let labels: Vec<String> = targets.iter().map(|t| t.label.clone()).collect();
    let Ok(Some(choice)) = select("Save where?", &labels, 0) else {
        return Ok(());
    };
    let target = &targets[choice];

    let block = b.to_recipe_toml(&name);
    let duplicate = save::append_recipe(&target.path, &name, &block)?;
    if duplicate {
        println!(
            "{}",
            style(format!(
                "  note: [recipe.{name}] already existed — appended another; \
                 edit to keep one"
            ))
            .yellow()
        );
    }
    println!(
        "{} {}",
        style("✓ saved").green(),
        style(format!("@{name} → {}", target.path.display())).dim()
    );
    println!(
        "{}",
        style(format!("  run it any time with: jlf @{name}")).dim()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{delete_word_back, parse_filter_line};

    #[test]
    fn parse_filter_line_keeps_spaced_values() {
        // A `~` value with spaces stays one filter.
        assert_eq!(
            parse_filter_line("fields.message~Build mode: DEBUG"),
            vec!["fields.message~Build mode: DEBUG"]
        );
        // Each `field op value` token starts a new filter.
        assert_eq!(parse_filter_line("level=INFO status>=500"), vec![
            "level=INFO",
            "status>=500"
        ]);
        // A spaced value followed by another filter.
        assert_eq!(parse_filter_line("msg~a b level=INFO"), vec![
            "msg~a b",
            "level=INFO"
        ]);
    }

    fn ctrl_w(s: &str) -> String {
        let mut buf: Vec<char> = s.chars().collect();
        let mut pos = buf.len();
        delete_word_back(&mut buf, &mut pos);
        let out: String = buf.iter().collect();
        assert_eq!(pos, buf.len());
        out
    }

    #[test]
    fn ctrl_w_stops_at_path_and_list_boundaries() {
        // Dotted path: peel one segment at a time, not the whole line. A
        // leading boundary is consumed with the word it precedes
        // (standard readline).
        assert_eq!(
            ctrl_w("level,fields.message,span.method"),
            "level,fields.message,span."
        );
        assert_eq!(
            ctrl_w("level,fields.message,span."),
            "level,fields.message,"
        );
        assert_eq!(ctrl_w("level,fields.message,"), "level,fields.");
        // Whitespace still works; underscores stay part of a word.
        assert_eq!(ctrl_w("ts level user_id"), "ts level ");
        assert_eq!(ctrl_w("user_id"), "");
    }
}
