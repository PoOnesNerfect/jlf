use std::fmt::Write as _;
use std::sync::mpsc::Receiver;

use jlf_core::{expanded_format, Filter, Formatter, Json};

use crate::field::{path, resolve, scalar};
use crate::summary::{self, Summary};

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
    pub detail_scroll: u16,
    pub quit: bool,

    rx: Receiver<String>,
    row_fmt: Formatter,
}

impl App {
    pub fn new(rx: Receiver<String>) -> color_eyre::Result<Self> {
        // Render list rows with the same default template the CLI uses, but
        // forced compact + uncolored so each record is a single plain line that
        // ratatui can lay out and highlight itself.
        let variables = match jlf_core::get_config() {
            Ok(cfg) => merge_variables(cfg.variables),
            Err(_) => jlf_core::default_variables(),
        };
        let expanded = expanded_format("{&output}", &variables);
        let row_fmt = Formatter::new(&expanded, true, true)?;

        Ok(Self {
            lines: Vec::new(),
            view: Vec::new(),
            filters: Vec::new(),
            redact: Vec::new(),
            selected: 0,
            follow: true,
            mode: Mode::Normal,
            input: String::new(),
            filter_text: String::new(),
            status: "j/k move · / filter · : command · f follow · q quit".into(),
            summary: None,
            detail_scroll: 0,
            quit: false,
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

    fn passes(&self, line: &str) -> bool {
        if self.filters.is_empty() {
            return true;
        }
        let mut j = Json::Null;
        match j.parse_replace(line) {
            Ok(()) => jlf_core::matches_all(&self.filters, &j),
            Err(_) => false,
        }
    }

    /// Re-evaluate the filter over all records (after the expression changes).
    fn rebuild_view(&mut self) {
        self.view.clear();
        for (i, line) in self.lines.iter().enumerate() {
            let matched = if self.filters.is_empty() {
                true
            } else {
                let mut j = Json::Null;
                j.parse_replace(line)
                    .map(|()| jlf_core::matches_all(&self.filters, &j))
                    .unwrap_or(false)
            };
            if matched {
                self.view.push(i);
            }
        }
        self.clamp_selection();
    }

    pub fn apply_filter(&mut self, text: String) {
        self.filters = text
            .split_whitespace()
            .filter_map(Filter::parse)
            .collect();
        self.filter_text = text;
        self.rebuild_view();
        self.status = if self.filters.is_empty() {
            "filter cleared".into()
        } else {
            format!("{} match", self.view.len())
        };
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
        out
    }

    /// Pretty-printed JSON of a record for the detail pane.
    pub fn render_detail(&self, line: &str) -> String {
        let mut j = Json::Null;
        if j.parse_replace(line).is_err() {
            return line.to_owned();
        }
        if !self.redact.is_empty() {
            jlf_core::redact(&mut j, &self.redact);
        }
        format!("{}", j.indented(2))
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
            other => self.status = format!("unknown command: {other}"),
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
