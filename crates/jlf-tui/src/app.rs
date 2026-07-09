use std::fmt::Write as _;
use std::sync::mpsc::Receiver;

use jlf_core::{expanded_format, Filter, Formatter, Json};

use crate::catalog::{self, Catalog};
use crate::field::{path, resolve, scalar};
use crate::summary::{self, Summary};

/// The `:` commands, offered as autocomplete when the command line is open.
pub const COMMANDS: [&str; 12] = [
    "count", "stats", "top", "uniq", "redact", "csv", "tsv", "md", "follow", "save", "help",
    "quit",
];

/// Entries in the Actions panel (opened with `a`): a label and the command it
/// runs. A prefix ending in a space opens the command line pre-filled so the
/// field / columns / name can be typed with autocomplete; the rest run at once.
pub const ACTIONS: [(&str, &str); 9] = [
    ("Count — total records", "count"),
    ("Count grouped by a field…", "count "),
    ("Stats of a numeric field…", "stats "),
    ("Top values of a field…", "top "),
    ("Unique values of a field…", "uniq "),
    ("Export as CSV…", "csv "),
    ("Export as TSV…", "tsv "),
    ("Export as Markdown…", "md "),
    ("Save current view as a recipe…", "save "),
];

/// Which input the keypresses are being routed to.
#[derive(PartialEq)]
pub enum Mode {
    Normal,
    /// Typing a filter expression (`/`).
    Search,
    /// Typing a `:` command.
    Command,
}

pub struct App {
    /// Every record received, raw (no trailing newline). Records are kept as
    /// text and parsed on demand — parsing is ~1µs/line, so re-rendering a
    /// screenful is free, and we avoid an owned JSON representation.
    pub lines: Vec<String>,
    /// Indices into `lines` that pass the current filter.
    pub view: Vec<usize>,
    filters: Vec<Filter>,
    /// Bare (operator-less) words typed into `/`: a record matches when its raw
    /// text contains all of them (case-insensitive) — a plain "search anywhere".
    search_terms: Vec<String>,
    redact: Vec<String>,

    /// Cursor position within `view`.
    pub selected: usize,
    /// Auto-scroll to the newest matching record as data streams in.
    pub follow: bool,

    pub mode: Mode,
    pub input: String,
    pub filter_text: String,
    pub status: String,
    pub summary: Option<Summary>,
    /// Whether the keys/commands help overlay is showing.
    pub help: bool,
    /// Whether the Actions panel is open, and its highlighted row.
    pub show_actions: bool,
    pub action_sel: usize,
    /// Whether the detail pane is open (toggled with Enter on a row).
    pub show_detail: bool,
    pub detail_scroll: u16,
    pub quit: bool,

    /// Field catalog for search autocomplete, rebuilt when entering search.
    catalog: Catalog,
    sug: Vec<String>,
    sug_start: usize,
    sug_sel: usize,
    sug_dismissed: bool,

    rx: Receiver<String>,
    /// Single-line, colored formatter for list rows.
    row_fmt: Formatter,
}

impl App {
    pub fn new(rx: Receiver<String>) -> color_eyre::Result<Self> {
        // Colored, compact list rows (ratatui renders the ANSI); the detail pane
        // renders syntax-highlighted pretty JSON directly.
        let compact_vars = match jlf_core::get_config() {
            Ok(mut cfg) => {
                cfg.resolve_recipes(&["compact"]);
                merge_variables(cfg.variables)
            }
            Err(_) => jlf_core::default_variables(),
        };
        let row_fmt = Formatter::new(&expanded_format("${@output}", &compact_vars), false, true)?;

        Ok(Self {
            lines: Vec::new(),
            view: Vec::new(),
            filters: Vec::new(),
            search_terms: Vec::new(),
            redact: Vec::new(),
            selected: 0,
            follow: true,
            mode: Mode::Normal,
            input: String::new(),
            filter_text: String::new(),
            status: String::new(),
            summary: None,
            help: false,
            show_actions: false,
            action_sel: 0,
            show_detail: false,
            detail_scroll: 0,
            quit: false,
            catalog: Catalog::default(),
            sug: Vec::new(),
            sug_start: 0,
            sug_sel: 0,
            sug_dismissed: false,
            rx,
            row_fmt,
        })
    }

    /// Drain any lines the reader thread produced since the last tick. Returns
    /// true if at least one new record arrived (so the caller can redraw).
    pub fn drain_input(&mut self) -> bool {
        let mut changed = false;
        while let Ok(line) = self.rx.try_recv() {
            let idx = self.lines.len();
            let matched = self.passes(&line);
            self.lines.push(line);
            if matched {
                self.view.push(idx);
            }
            changed = true;
        }
        if changed && self.follow {
            self.jump_to_bottom();
        }
        changed
    }

    /// Whether `line` passes the active structured filters and plain-text search
    /// terms. Empty criteria pass everything.
    fn passes(&self, line: &str) -> bool {
        if !self.matches_search(line) {
            return false;
        }
        if self.filters.is_empty() {
            return true;
        }
        let mut j = Json::Null;
        match j.parse_replace(line) {
            Ok(()) => jlf_core::matches_all(&self.filters, &j),
            Err(_) => false,
        }
    }

    /// Every bare search term must appear (case-insensitive) in the raw record.
    fn matches_search(&self, line: &str) -> bool {
        if self.search_terms.is_empty() {
            return true;
        }
        let hay = line.to_lowercase();
        self.search_terms.iter().all(|t| hay.contains(t))
    }

    /// Re-evaluate the filter over all records (after the expression changes).
    fn rebuild_view(&mut self) {
        self.view.clear();
        for i in 0..self.lines.len() {
            if self.passes(&self.lines[i]) {
                self.view.push(i);
            }
        }
        self.clamp_selection();
    }

    pub fn apply_filter(&mut self, text: String) {
        // Tokens that parse as `field op value` are structured filters; bare
        // words become plain-text search terms matched against the raw record.
        self.filters.clear();
        self.search_terms.clear();
        for tok in text.split_whitespace() {
            match Filter::parse(tok) {
                Some(f) => self.filters.push(f),
                None => self.search_terms.push(tok.to_lowercase()),
            }
        }
        self.filter_text = text;
        self.rebuild_view();
        self.status = if self.filters.is_empty() && self.search_terms.is_empty() {
            "filter cleared".into()
        } else {
            format!("{} match", self.view.len())
        };
    }

    // ----- search autocomplete ---------------------------------------------

    /// Enter search mode: seed the input from the active filter and build the
    /// field catalog from the loaded records.
    pub fn enter_search(&mut self) {
        self.mode = Mode::Search;
        self.input = self.filter_text.clone();
        self.catalog = Catalog::from_lines(&self.lines, 2000);
        self.sug_dismissed = false;
        self.sug_sel = 0;
        self.refresh_suggestions();
    }

    /// Enter command mode: start empty and immediately offer the command list so
    /// the available commands are discoverable.
    pub fn enter_command(&mut self) {
        self.enter_command_with("");
    }

    /// Enter command mode pre-filled with `prefix` (used by the Actions panel so
    /// e.g. picking "Stats" drops you into `:stats ` with field autocomplete).
    fn enter_command_with(&mut self, prefix: &str) {
        self.mode = Mode::Command;
        self.input = prefix.to_owned();
        self.catalog = Catalog::from_lines(&self.lines, 2000);
        self.sug_dismissed = false;
        self.sug_sel = 0;
        self.refresh_suggestions();
    }

    // ----- actions panel ----------------------------------------------------

    pub fn open_actions(&mut self) {
        self.show_actions = true;
        self.action_sel = 0;
    }

    pub fn action_move(&mut self, delta: isize) {
        let n = ACTIONS.len() as isize;
        self.action_sel = (((self.action_sel as isize + delta) % n + n) % n) as usize;
    }

    /// Run the highlighted action: immediate ones (plain `count`) execute now;
    /// the rest open the command line pre-filled for a field / columns / name.
    pub fn run_action(&mut self) {
        self.show_actions = false;
        let (_, cmd) = ACTIONS[self.action_sel.min(ACTIONS.len() - 1)];
        if let Some(prefix) = cmd.strip_suffix(' ') {
            self.enter_command_with(&format!("{prefix} "));
        } else {
            self.run_command(cmd);
        }
    }

    fn refresh_suggestions(&mut self) {
        let (start, cands) = match self.mode {
            Mode::Command => catalog::command_suggest(&self.input, &self.catalog, &COMMANDS),
            _ => catalog::suggest(&self.input, &self.catalog),
        };
        self.sug_start = start;
        self.sug = cands;
        if self.sug_sel >= self.sug.len() {
            self.sug_sel = 0;
        }
    }

    pub fn input_char(&mut self, c: char) {
        self.input.push(c);
        self.after_input_change();
    }

    pub fn input_backspace(&mut self) {
        self.input.pop();
        self.after_input_change();
    }

    /// Delete the word before the cursor (Ctrl-W).
    pub fn input_delete_word(&mut self) {
        let cut = {
            let trimmed = self.input.trim_end_matches(char::is_whitespace);
            trimmed.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0)
        };
        self.input.truncate(cut);
        self.after_input_change();
    }

    fn after_input_change(&mut self) {
        self.sug_sel = 0;
        self.sug_dismissed = false;
        if matches!(self.mode, Mode::Search | Mode::Command) {
            self.refresh_suggestions();
        }
    }

    pub fn suggestion_move(&mut self, delta: isize) {
        if self.sug.is_empty() {
            return;
        }
        self.sug_dismissed = false;
        let n = self.sug.len() as isize;
        self.sug_sel = (((self.sug_sel as isize + delta) % n + n) % n) as usize;
    }

    /// Fill the highlighted suggestion into the input. Returns whether it did.
    pub fn fill_suggestion(&mut self) -> bool {
        if self.sug_dismissed || self.sug.is_empty() {
            return false;
        }
        let c = self.sug[self.sug_sel.min(self.sug.len() - 1)].clone();
        self.input.truncate(self.sug_start);
        self.input.push_str(&c);
        self.sug_sel = 0;
        self.refresh_suggestions();
        true
    }

    pub fn dismiss_suggestions(&mut self) {
        self.sug_dismissed = true;
    }

    pub fn suggestions_visible(&self) -> bool {
        matches!(self.mode, Mode::Search | Mode::Command)
            && !self.sug_dismissed
            && !self.sug.is_empty()
    }

    pub fn suggestions(&self) -> (&[String], usize) {
        (&self.sug, self.sug_sel)
    }

    // ----- navigation -------------------------------------------------------

    fn clamp_selection(&mut self) {
        let max = self.view.len().saturating_sub(1);
        self.selected = self.selected.min(max);
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let new = (self.selected as isize + delta).clamp(0, self.view.len() as isize - 1);
        self.selected = new as usize;
        // Moving away from the newest record stops follow; reaching the end
        // re-enables it.
        self.follow = self.selected + 1 == self.view.len();
        self.detail_scroll = 0;
    }

    pub fn jump_to_top(&mut self) {
        self.selected = 0;
        self.follow = false;
        self.detail_scroll = 0;
    }

    pub fn jump_to_bottom(&mut self) {
        self.selected = self.view.len().saturating_sub(1);
        self.follow = true;
        self.detail_scroll = 0;
    }

    pub fn toggle_follow(&mut self) {
        self.follow = !self.follow;
        if self.follow {
            self.jump_to_bottom();
        }
        self.status = if self.follow { "following" } else { "paused" }.into();
    }

    // ----- selected record --------------------------------------------------

    pub fn selected_line(&self) -> Option<&str> {
        self.view.get(self.selected).map(|&i| self.lines[i].as_str())
    }

    /// One-line rendering of a record for the list (parses, redacts, formats).
    /// Any newlines the template emits are collapsed so each record occupies
    /// exactly one row — otherwise the list windowing (one item = one row)
    /// mis-counts and leaves a stray blank row while scrolling.
    pub fn render_row(&self, line: &str) -> String {
        let mut j = Json::Null;
        if j.parse_replace(line).is_err() {
            return line.to_owned();
        }
        if !self.redact.is_empty() {
            jlf_core::redact(&mut j, &self.redact);
        }
        let mut out = String::new();
        if self.row_fmt.as_log(&j).write_fmt(&mut out).is_err() {
            return line.to_owned();
        }
        out.replace('\n', "  ")
    }

    /// Full, colored rendering of a record for the detail pane: syntax-
    /// highlighted pretty JSON so every field is visible and easy to read.
    pub fn render_detail(&self, line: &str) -> String {
        let mut j = Json::Null;
        if j.parse_replace(line).is_err() {
            return line.to_owned();
        }
        if !self.redact.is_empty() {
            jlf_core::redact(&mut j, &self.redact);
        }
        format!("{:?}", j.styled(jlf_core::MarkupStyles::default()).indented(2))
    }

    fn view_lines(&self) -> Vec<&str> {
        self.view.iter().map(|&i| self.lines[i].as_str()).collect()
    }

    // ----- commands ---------------------------------------------------------

    pub fn run_command(&mut self, cmd: &str) {
        let mut it = cmd.split_whitespace();
        let Some(verb) = it.next() else { return };
        let rest: Vec<&str> = it.collect();
        match verb {
            "q" | "quit" => self.quit = true,
            "count" => {
                let lines = self.view_lines();
                self.summary = Some(summary::count(&lines, rest.first().copied()));
            }
            "uniq" => match rest.first() {
                Some(f) => {
                    let lines = self.view_lines();
                    self.summary = Some(summary::uniq(&lines, f));
                }
                None => self.status = "uniq needs a field".into(),
            },
            "stats" => match rest.first() {
                Some(f) => {
                    let lines = self.view_lines();
                    self.summary = Some(summary::stats(&lines, f));
                }
                None => self.status = "stats needs a field".into(),
            },
            "top" => match rest.first() {
                Some(f) => {
                    let n = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
                    let lines = self.view_lines();
                    self.summary = Some(summary::top(&lines, f, n));
                }
                None => self.status = "top needs a field".into(),
            },
            "redact" => {
                self.redact = rest
                    .first()
                    .map(|s| s.split(',').map(str::to_owned).collect())
                    .unwrap_or_default();
                self.status = if self.redact.is_empty() {
                    "redaction cleared".into()
                } else {
                    format!("redacting {}", self.redact.join(", "))
                };
            }
            "follow" => self.toggle_follow(),
            "csv" | "tsv" | "md" => self.export(verb, &rest),
            "save" => match rest.first() {
                Some(name) => match crate::save::save_recipe(name, &self.filter_text, &self.redact) {
                    Ok((path, dup)) => {
                        let note = if dup { " (name already existed)" } else { "" };
                        self.status = format!("saved recipe '{name}' to {}{note}", path.display());
                    }
                    Err(e) => self.status = format!("save failed: {e}"),
                },
                None => self.status = "save needs a recipe name".into(),
            },
            "help" | "h" | "?" => self.help = true,
            other => {
                self.status =
                    format!("unknown command '{other}' — try: {} (? for help)", COMMANDS.join(" "));
            }
        }
    }

    fn export(&mut self, kind: &str, rest: &[&str]) {
        let Some(cols) = rest.first() else {
            self.status = format!("{kind} needs columns, e.g. :{kind} ts,level,msg");
            return;
        };
        let cols: Vec<Vec<String>> = cols.split(',').map(path).collect();
        let out_path = rest
            .get(1)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("jlf-export.{kind}"));

        let sep = if kind == "tsv" { '\t' } else { ',' };
        let md = kind == "md";
        let mut buf = String::new();
        let header: Vec<String> = cols
            .iter()
            .map(|c| c.join("."))
            .collect();
        write_row(&mut buf, &header, sep, md);
        if md {
            let dashes: Vec<String> = header.iter().map(|_| "---".to_owned()).collect();
            write_row(&mut buf, &dashes, sep, md);
        }
        for &i in &self.view {
            let mut j = Json::Null;
            if j.parse_replace(&self.lines[i]).is_err() {
                continue;
            }
            let cells: Vec<String> = cols
                .iter()
                .map(|c| scalar(resolve(&j, c)).unwrap_or("").to_owned())
                .collect();
            write_row(&mut buf, &cells, sep, md);
        }
        match std::fs::write(&out_path, buf) {
            Ok(()) => self.status = format!("wrote {} rows to {out_path}", self.view.len()),
            Err(e) => self.status = format!("export failed: {e}"),
        }
    }
}

fn write_row(buf: &mut String, cells: &[String], sep: char, md: bool) {
    if md {
        let _ = writeln!(buf, "| {} |", cells.join(" | "));
    } else {
        let escaped: Vec<String> = cells
            .iter()
            .map(|c| {
                if c.contains(sep) || c.contains('"') || c.contains('\n') {
                    format!("\"{}\"", c.replace('"', "\"\""))
                } else {
                    c.clone()
                }
            })
            .collect();
        let _ = writeln!(buf, "{}", escaped.join(&sep.to_string()));
    }
}

/// Merge the user's configured variables (if any) over the built-in defaults.
fn merge_variables(from_config: Option<Vec<(String, String)>>) -> Vec<(String, String)> {
    let mut variables = jlf_core::default_variables();
    if let Some(from_config) = from_config {
        for (k2, v2) in from_config {
            match variables.iter_mut().find(|(k, _)| k == &k2) {
                Some((_, v)) => *v = v2,
                None => variables.push((k2, v2)),
            }
        }
    }
    variables
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn app_with(lines: &[&str]) -> App {
        let (tx, rx) = channel();
        for l in lines {
            tx.send((*l).to_string()).unwrap();
        }
        drop(tx);
        let mut app = App::new(rx).unwrap();
        app.drain_input();
        app
    }

    const SAMPLE: &[&str] = &[
        r#"{"level":"info","user":"alice","latency_ms":42}"#,
        r#"{"level":"error","user":"bob","latency_ms":510}"#,
        r#"{"level":"warn","user":"alice","latency_ms":88}"#,
    ];

    #[test]
    fn ingests_all_records_into_view() {
        let app = app_with(SAMPLE);
        assert_eq!(app.lines.len(), 3);
        assert_eq!(app.view.len(), 3);
    }

    #[test]
    fn filter_narrows_the_view() {
        let mut app = app_with(SAMPLE);
        app.apply_filter("level=error,warn".into());
        assert_eq!(app.view.len(), 2);
        app.apply_filter(String::new());
        assert_eq!(app.view.len(), 3);
    }

    #[test]
    fn follow_tracks_bottom_until_user_scrolls_up() {
        let mut app = app_with(SAMPLE);
        assert!(app.follow);
        assert_eq!(app.selected, 2);
        app.move_by(-1);
        assert!(!app.follow);
        assert_eq!(app.selected, 1);
        app.jump_to_bottom();
        assert!(app.follow);
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn new_records_append_to_filtered_view_live() {
        let (tx, rx) = channel();
        let mut app = App::new(rx).unwrap();
        // drive a live stream through the same channel the reader would use.
        let send = |s: &str| tx.send(s.to_string()).unwrap();
        send(SAMPLE[0]);
        app.drain_input();
        app.apply_filter("level=error".into());
        assert_eq!(app.view.len(), 0);
        send(SAMPLE[1]); // an error arrives after the filter is set
        app.drain_input();
        assert_eq!(app.view.len(), 1);
    }

    #[test]
    fn count_command_builds_a_summary() {
        let mut app = app_with(SAMPLE);
        app.run_command("count level");
        let s = app.summary.as_ref().unwrap();
        assert_eq!(s.title, "count level");
        assert!(s.rows.iter().any(|r| r.contains("total")));
    }

    #[test]
    fn stats_command_over_numeric_field() {
        let mut app = app_with(SAMPLE);
        app.run_command("stats latency_ms");
        let s = app.summary.as_ref().unwrap();
        assert!(s.rows.iter().any(|r| r == "count 3"));
        assert!(s.rows.iter().any(|r| r.contains("max   510")));
    }

    #[test]
    fn csv_export_writes_filtered_rows() {
        let mut app = app_with(SAMPLE);
        app.apply_filter("level=error".into());
        let path = std::env::temp_dir().join(format!("jlf_tui_test_{}.csv", std::process::id()));
        app.run_command(&format!("csv level,user {}", path.display()));
        let out = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(out, "level,user\nerror,bob\n");
    }

    #[test]
    fn redact_masks_values_in_rendering() {
        let mut app = app_with(&[r#"{"user":"alice","token":"secret"}"#]);
        app.run_command("redact token");
        let row = app.render_row(app.selected_line().unwrap());
        assert!(row.contains("***"));
        assert!(!row.contains("secret"));
    }
}
