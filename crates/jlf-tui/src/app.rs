use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::mpsc::Receiver;

use jlf_core::{expanded_format, Filter, Formatter, Json};

use crate::catalog::{self, Catalog};
use crate::field::{path, resolve, scalar};
use crate::store::Store;
use crate::summary::{Agg, Summary};

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
    /// Typing a filter expression (`?`) — narrows the view.
    Filter,
    /// Typing a search query (`/`) — highlights matches without hiding rows.
    Search,
    /// Typing a `:` command.
    Command,
}

/// A frozen Tab-cycle over a suggestion list. `cands` is captured when cycling
/// begins so filling successive items doesn't collapse the list; `base` is the
/// text the user had typed (restored when the cycle steps past either end and
/// deselects); each step truncates the input back to `start` and appends the
/// selected candidate (or `base`).
struct Cycle {
    start: usize,
    base: String,
    cands: Vec<String>,
    idx: usize,
}

/// Chars that separate words for word-wise cursor motion and Ctrl-W. Matches how
/// filters/command args tokenize (whitespace plus `,`/`.`/operator chars), so a
/// word jump lands on field/value boundaries like `fields.status` or `a>5,b`.
fn is_word_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | '.' | '=' | '~' | '>' | '<' | '!' | ':' | '|')
}

/// How many records to fold per `tick_summary` call, so a summary over a huge
/// store progresses across frames instead of freezing the UI.
const SUMMARY_BATCH: usize = 50_000;

/// How many newly-arrived records to ingest per frame. Bounds the per-frame work
/// during a burst (opening a huge file) so the UI keeps painting — the record
/// count climbs visibly as a natural progress indicator — instead of blocking one
/// frame until the whole stream is read. ~50k keeps a frame's ingest well under
/// ~10ms even with disk spilling.
const DRAIN_BATCH: usize = 50_000;

/// The largest view (in records) for which search shows a match count. Beyond
/// this a full scan would page the spilled store from disk, so the count is
/// omitted rather than computed (or partially — and misleadingly — scanned);
/// `n`/`N` navigation is incremental and still works at any size.
const COUNT_SCAN_CAP: usize = 100_000;

/// The search match count for the status line.
#[derive(Debug, PartialEq)]
pub enum MatchCount {
    /// Scanned exactly: the current 1-based match index (when the selection is on
    /// a match) and the total number of matches.
    Counted { current: Option<usize>, total: usize },
    /// The view was too large ([`COUNT_SCAN_CAP`]) to count cheaply.
    Uncounted,
}

/// Smart-case: a search is case-sensitive when its query contains any uppercase
/// letter, and case-insensitive when it's all lowercase — the ripgrep/vim
/// convention. A case-sensitive query can use the fast SIMD substring finder.
pub(crate) fn search_case_sensitive(query: &str) -> bool {
    query.chars().any(|c| c.is_uppercase())
}

/// A prepared substring matcher for a search query, built once per scan and
/// reused across records. Smart-case: an all-lowercase query matches case-
/// insensitively (lowercasing each record); a query with any uppercase matches
/// exactly via a SIMD [`memmem::Finder`], which is both faster and more precise.
enum SearchMatcher {
    Sensitive(memchr::memmem::Finder<'static>),
    Insensitive(String),
}

impl SearchMatcher {
    fn new(query: &str) -> Self {
        if search_case_sensitive(query) {
            Self::Sensitive(memchr::memmem::Finder::new(query).into_owned())
        } else {
            Self::Insensitive(query.to_lowercase())
        }
    }

    fn is_match(&self, hay: &str) -> bool {
        match self {
            Self::Sensitive(finder) => finder.find(hay.as_bytes()).is_some(),
            Self::Insensitive(lower) => hay.to_lowercase().contains(lower.as_str()),
        }
    }
}

/// A running summary: the aggregator plus how far through the view it has folded
/// (a view position). New records past `cursor` are picked up on later ticks.
struct SummaryJob {
    agg: Agg,
    cursor: usize,
}

pub struct App {
    /// Bounded, file-backed record store: recent and oldest records stay in
    /// memory, the middle spills to a temp file and is paged back on demand.
    store: Store,
    /// Logical record indices passing the current filter, or `None` when
    /// unfiltered — then the view is the identity `0..len`, kept implicit so the
    /// common (no-filter) case costs no per-record memory.
    view: Option<Vec<usize>>,
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
    /// Cursor position within `input`, as a char index (0..=chars). Editing and
    /// motion (arrows, word jumps, Home/End) all act relative to it.
    pub input_cursor: usize,
    pub filter_text: String,
    /// The active search query (`/`): matched text is highlighted in the list and
    /// `n`/`N` jump between matching records. Empty means no search.
    pub search_query: String,
    /// Cached view positions matching `search_query`, sorted ascending, with the
    /// `(query, view_len)` they were computed for. Rebuilt once per search (or
    /// when the view changes) so `n`/`N` and the `k/total` position are cheap
    /// lookups rather than a per-frame full scan.
    search_matches: Vec<usize>,
    search_matches_key: (String, usize),
    pub status: String,
    /// The rendered summary panel, if one is open.
    pub summary: Option<Summary>,
    /// The running aggregator behind `summary`: it folds the view incrementally
    /// (across frames for a huge store) and keeps updating as new records arrive.
    summary_job: Option<SummaryJob>,
    /// Whether the keys/commands help overlay is showing.
    pub help: bool,
    /// Whether the Actions panel is open, and its highlighted row.
    pub show_actions: bool,
    pub action_sel: usize,
    /// Whether the detail pane is open (toggled with Enter on a row).
    pub show_detail: bool,
    pub detail_scroll: u16,
    pub quit: bool,
    /// Set to force a full repaint next frame (Ctrl-L) — recovers the display if
    /// something outside our control (e.g. a producer logging to the terminal's
    /// stderr) has corrupted it.
    pub force_redraw: bool,
    /// Set when `e` is pressed; the run loop opens the current view in $EDITOR
    /// (it owns the terminal it must suspend) and clears the flag.
    pub pending_editor: bool,

    /// Field catalog for search autocomplete, rebuilt when entering search.
    catalog: Catalog,
    sug: Vec<String>,
    sug_start: usize,
    /// Active Tab-cycle over a frozen candidate list, or None when nothing is
    /// selected (the initial state, and after any edit).
    sug_cycle: Option<Cycle>,

    rx: Receiver<String>,
    /// Single-line, colored formatter for compact list rows.
    row_fmt: Formatter,
    /// Multi-line, colored formatter for the expanded list (like piped `jlf`).
    full_fmt: Formatter,
    /// When true, the list shows each record over multiple lines (header +
    /// pretty data) like piped `jlf`; toggled with `c`.
    pub expanded: bool,
    /// When true, the list shows the raw record (the JSON as it arrived) instead
    /// of the recipe-formatted output; toggled with `r`. Combines with `expanded`:
    /// compact shows the raw one-liner, expanded shows pretty-printed JSON.
    pub raw: bool,
    /// Index of the first visible record. Persisted across frames so the
    /// viewport scrolls with a margin (see the list renderer) instead of pinning
    /// the cursor to an edge. Updated at draw time, where the height is known.
    pub scroll_top: std::cell::Cell<usize>,
    /// Number of records visible in the last frame, so `d`/`u`/`D`/`U` can page
    /// by the real viewport size. Set at draw time.
    pub page: std::cell::Cell<usize>,
}

impl App {
    pub fn new(rx: Receiver<String>) -> color_eyre::Result<Self> {
        // Colored list rows (ratatui renders the ANSI); the detail pane renders
        // syntax-highlighted pretty JSON directly. Two row formatters: the
        // `compact` recipe override collapses records to one line, the default
        // spans multiple lines exactly like piped `jlf`.
        let vars = |flags: &[&str]| match jlf_core::get_config() {
            Ok(mut cfg) => {
                cfg.resolve_recipes(flags);
                merge_variables(cfg.variables)
            }
            Err(_) => jlf_core::default_variables(),
        };
        let row_fmt = Formatter::new(&expanded_format("${@output}", &vars(&["compact"])), false, true)?;
        let full_fmt = Formatter::new(&expanded_format("${@output}", &vars(&[])), false, false)?;

        // Start in the mode the config asks for: `compact = true` opens in the
        // one-line view, otherwise the multi-line (expanded) view. `c` toggles it.
        let compact = jlf_core::get_config()
            .ok()
            .and_then(|c| c.config.compact)
            .unwrap_or(false);

        Ok(Self {
            store: Store::new(),
            view: None,
            filters: Vec::new(),
            search_terms: Vec::new(),
            redact: Vec::new(),
            selected: 0,
            follow: true,
            mode: Mode::Normal,
            input: String::new(),
            input_cursor: 0,
            filter_text: String::new(),
            search_query: String::new(),
            search_matches: Vec::new(),
            search_matches_key: (String::new(), 0),
            status: String::new(),
            summary: None,
            summary_job: None,
            help: false,
            show_actions: false,
            action_sel: 0,
            show_detail: false,
            detail_scroll: 0,
            quit: false,
            force_redraw: false,
            pending_editor: false,
            catalog: Catalog::default(),
            sug: Vec::new(),
            sug_start: 0,
            sug_cycle: None,
            rx,
            row_fmt,
            full_fmt,
            expanded: !compact,
            raw: false,
            scroll_top: std::cell::Cell::new(0),
            page: std::cell::Cell::new(1),
        })
    }

    /// Drain any lines the reader thread produced since the last tick. Returns
    /// true if at least one new record arrived (so the caller can redraw).
    /// Ingest newly-arrived records, at most [`DRAIN_BATCH`] per call so a burst
    /// (e.g. opening a million-line file) fills the view progressively across
    /// frames instead of blocking one frame for the whole stream. Returns
    /// `(changed, more_pending)`: `more_pending` is true when the batch cap was
    /// hit and the caller should loop again promptly rather than idle.
    pub fn drain_input(&mut self) -> (bool, bool) {
        let mut changed = false;
        let mut count = 0;
        while count < DRAIN_BATCH {
            let Ok(line) = self.rx.try_recv() else {
                break;
            };
            let idx = self.store.len();
            let matched = self.passes(&line);
            self.store.push(line);
            // Keep an explicit filtered view in sync; the unfiltered view is
            // implicit (identity), so nothing to track there.
            if let Some(v) = &mut self.view {
                if matched {
                    v.push(idx);
                }
            }
            changed = true;
            count += 1;
        }
        if changed && self.follow {
            self.jump_to_bottom();
        }
        (changed, count == DRAIN_BATCH)
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
    /// With no criteria the view is left implicit (identity); otherwise it's
    /// materialized by scanning the store (paging the spilled middle back in).
    fn rebuild_view(&mut self) {
        if self.filters.is_empty() && self.search_terms.is_empty() {
            self.view = None;
        } else {
            let mut v = Vec::new();
            for i in 0..self.store.len() {
                if self.passes(&self.store.get(i)) {
                    v.push(i);
                }
            }
            self.view = Some(v);
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
        // The view changed, so the cached search matches (view positions) are
        // stale — recompute them against the new view.
        if !self.search_query.is_empty() {
            self.refresh_search_matches();
        }
        // A change in the view invalidates a running summary — recompute it from
        // scratch over the new view.
        if let Some(job) = &mut self.summary_job {
            job.agg.reset();
            job.cursor = 0;
        }
        self.tick_summary();
        self.status = if self.filters.is_empty() && self.search_terms.is_empty() {
            "filter cleared".into()
        } else {
            format!("{} match", self.view_len())
        };
    }

    // ----- record access ----------------------------------------------------

    /// Total records held (across memory and the spill file).
    pub fn total(&self) -> usize {
        self.store.len()
    }

    /// Number of records in the current view.
    pub fn view_len(&self) -> usize {
        self.view.as_ref().map_or(self.store.len(), Vec::len)
    }

    /// The logical store index for view position `pos`, if in range.
    fn view_index(&self, pos: usize) -> Option<usize> {
        match &self.view {
            Some(v) => v.get(pos).copied(),
            None => (pos < self.store.len()).then_some(pos),
        }
    }

    /// The record at view position `pos` (paging from disk if needed).
    pub fn record(&self, pos: usize) -> Option<Rc<str>> {
        self.view_index(pos).map(|i| self.store.get(i))
    }

    /// Preload the record at view position `pos` so a later render is a cache
    /// hit (used to prefetch just off the visible edges).
    pub fn prefetch(&self, pos: usize) {
        if let Some(i) = self.view_index(pos) {
            self.store.prefetch(i);
        }
    }

    /// The most recent `n` records, oldest-first (for autocomplete catalogs).
    fn recent(&self, n: usize) -> Vec<Rc<str>> {
        let len = self.store.len();
        (len.saturating_sub(n)..len).map(|i| self.store.get(i)).collect()
    }

    // ----- search autocomplete ---------------------------------------------

    /// Enter filter mode (`?`): seed the input from the active filter and build
    /// the field catalog from the loaded records so completion works.
    pub fn enter_filter(&mut self) {
        self.mode = Mode::Filter;
        self.input = self.filter_text.clone();
        self.input_cursor = self.input.chars().count();
        let recent = self.recent(2000);
        self.catalog = Catalog::from_lines(&recent, 2000);
        self.sug_cycle = None;
        self.refresh_suggestions();
    }

    /// Enter search mode (`/`): seed the input from the active query. Search
    /// highlights matches and jumps between them without hiding rows.
    pub fn enter_search(&mut self) {
        self.mode = Mode::Search;
        self.input = self.search_query.clone();
        self.input_cursor = self.input.chars().count();
        self.sug_cycle = None;
        self.sug.clear();
    }

    /// Commit `query` as the active search and jump to the first match at or
    /// after the current selection. An empty query clears the search.
    pub fn apply_search(&mut self, query: String) {
        self.search_query = query;
        if self.search_query.is_empty() {
            self.clear_search_matches();
            self.status = "search cleared".into();
            return;
        }
        self.refresh_search_matches();
        // Jump to the first match from the end — the newest matching record at or
        // above the selection (logs read newest-last, so you usually open search
        // at the bottom and want the most recent hit). Stay put if the current
        // record already matches.
        let matcher = SearchMatcher::new(&self.search_query);
        if self.record_matches_search(self.selected, &matcher) {
            self.status = "search set".into();
        } else if let Some(pos) = self.find_match(self.selected, false) {
            self.select(pos);
            self.status = "search set".into();
        } else {
            self.status = "no matches".into();
        }
    }

    /// The text currently driving highlighting: the live input while typing a
    /// search, otherwise the committed query. Empty when neither is active.
    pub fn search_needle(&self) -> &str {
        match self.mode {
            Mode::Search => &self.input,
            _ => &self.search_query,
        }
    }

    /// Whether `pos`'s record matches `matcher` (search text anywhere in the raw
    /// record — any key or value).
    fn record_matches_search(&self, pos: usize, matcher: &SearchMatcher) -> bool {
        self.record(pos).is_some_and(|rec| matcher.is_match(&rec))
    }

    /// The next/previous view position matching the search, scanning outward from
    /// `from` and wrapping around. Incremental (stops at the first hit), so `n`/`N`
    /// stay cheap and reach every match regardless of the view size.
    fn find_match(&self, from: usize, forward: bool) -> Option<usize> {
        let len = self.view_len();
        if len == 0 || self.search_query.is_empty() {
            return None;
        }
        let matcher = SearchMatcher::new(&self.search_query);
        // Walk all `len` other positions in order, wrapping, so a match anywhere
        // is found (and `from` itself is the last resort on a full wrap).
        (1..=len)
            .map(|d| {
                let off = if forward { d } else { len - d };
                (from + off) % len
            })
            .find(|&p| self.record_matches_search(p, &matcher))
    }

    /// Rebuild the cached match list for the count/position indicator, but only
    /// when the view is small enough to scan cheaply ([`COUNT_SCAN_CAP`]). For a
    /// larger view a full scan would page the spilled store from disk (and a
    /// *partial* scan would miscount, e.g. when the matches are all in the tail),
    /// so the count is simply omitted — `n`/`N` navigation still works. Cached by
    /// `(query, view_len)` so it runs at most once per search/view change.
    fn refresh_search_matches(&mut self) {
        let key = (self.search_query.clone(), self.view_len());
        if self.search_matches_key == key {
            return;
        }
        self.search_matches = if self.search_query.is_empty() || self.view_len() > COUNT_SCAN_CAP {
            Vec::new()
        } else {
            let matcher = SearchMatcher::new(&self.search_query);
            (0..self.view_len())
                .filter(|&p| self.record_matches_search(p, &matcher))
                .collect()
        };
        self.search_matches_key = key;
    }

    fn clear_search_matches(&mut self) {
        self.search_matches.clear();
        self.search_matches_key = (String::new(), 0);
    }

    /// Jump to the next (`n`) or previous (`N`) matching record, wrapping around.
    pub fn search_jump(&mut self, forward: bool) {
        if self.search_query.is_empty() {
            self.status = "no active search".into();
            return;
        }
        match self.find_match(self.selected, forward) {
            Some(pos) => self.select(pos),
            None => self.status = "no matches".into(),
        }
    }

    /// The match count for the status line, or `None` when there's no active
    /// search. `Counted` carries `(current 1-based index if on a match, total)`;
    /// `Uncounted` means the view was too large to scan cheaply. Reads the cached
    /// list — no scan.
    pub fn search_position(&self) -> Option<MatchCount> {
        if self.search_query.is_empty() {
            return None;
        }
        if self.view_len() > COUNT_SCAN_CAP {
            return Some(MatchCount::Uncounted);
        }
        let current = self.search_matches.binary_search(&self.selected).ok().map(|i| i + 1);
        Some(MatchCount::Counted {
            current,
            total: self.search_matches.len(),
        })
    }

    /// Move the selection to view position `pos`, updating follow/detail state
    /// the same way keyboard navigation does.
    fn select(&mut self, pos: usize) {
        if self.view_len() == 0 {
            return;
        }
        self.selected = pos.min(self.view_len() - 1);
        self.follow = self.selected + 1 == self.view_len();
        self.detail_scroll = 0;
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
        self.input_cursor = self.input.chars().count();
        let recent = self.recent(2000);
        self.catalog = Catalog::from_lines(&recent, 2000);
        self.sug_cycle = None;
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
    }

    pub fn input_char(&mut self, c: char) {
        let at = self.byte_at(self.input_cursor);
        self.input.insert(at, c);
        self.input_cursor += 1;
        self.after_input_change();
    }

    /// Delete the char before the cursor (Backspace). No-op at the start.
    pub fn input_backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let end = self.byte_at(self.input_cursor);
        let start = self.byte_at(self.input_cursor - 1);
        self.input.replace_range(start..end, "");
        self.input_cursor -= 1;
        self.after_input_change();
    }

    /// Delete the char under the cursor (Delete / Ctrl-D). No-op at the end.
    pub fn input_delete_forward(&mut self) {
        if self.input_cursor >= self.input_len() {
            return;
        }
        let start = self.byte_at(self.input_cursor);
        let end = self.byte_at(self.input_cursor + 1);
        self.input.replace_range(start..end, "");
        self.after_input_change();
    }

    /// Delete the word before the cursor (Ctrl-W).
    pub fn input_delete_word(&mut self) {
        let target = self.word_left(self.input_cursor);
        let start = self.byte_at(target);
        let end = self.byte_at(self.input_cursor);
        self.input.replace_range(start..end, "");
        self.input_cursor = target;
        self.after_input_change();
    }

    /// Delete from the start of the line to the cursor (Ctrl-U).
    pub fn input_delete_to_start(&mut self) {
        let end = self.byte_at(self.input_cursor);
        self.input.replace_range(..end, "");
        self.input_cursor = 0;
        self.after_input_change();
    }

    /// Delete from the cursor to the end of the line (Ctrl-K).
    pub fn input_delete_to_end(&mut self) {
        let start = self.byte_at(self.input_cursor);
        self.input.truncate(start);
        self.after_input_change();
    }

    // ----- input cursor motion ---------------------------------------------

    /// Number of chars in the input.
    fn input_len(&self) -> usize {
        self.input.chars().count()
    }

    /// Byte offset of char index `n` (clamped to the input length).
    fn byte_at(&self, n: usize) -> usize {
        self.input
            .char_indices()
            .nth(n)
            .map(|(b, _)| b)
            .unwrap_or(self.input.len())
    }

    /// The char index one word to the left of `from` (skips trailing separators,
    /// then the word run). Words break on whitespace and `,`/`.`/operator chars,
    /// matching how filters and command args are tokenized.
    fn word_left(&self, from: usize) -> usize {
        let chars: Vec<char> = self.input.chars().collect();
        let mut i = from;
        while i > 0 && is_word_break(chars[i - 1]) {
            i -= 1;
        }
        while i > 0 && !is_word_break(chars[i - 1]) {
            i -= 1;
        }
        i
    }

    /// The char index one word to the right of `from`.
    fn word_right(&self, from: usize) -> usize {
        let chars: Vec<char> = self.input.chars().collect();
        let n = chars.len();
        let mut i = from;
        while i < n && is_word_break(chars[i]) {
            i += 1;
        }
        while i < n && !is_word_break(chars[i]) {
            i += 1;
        }
        i
    }

    pub fn input_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
        self.sug_cycle = None;
    }

    pub fn input_right(&mut self) {
        self.input_cursor = (self.input_cursor + 1).min(self.input_len());
        self.sug_cycle = None;
    }

    pub fn input_word_left(&mut self) {
        self.input_cursor = self.word_left(self.input_cursor);
        self.sug_cycle = None;
    }

    pub fn input_word_right(&mut self) {
        self.input_cursor = self.word_right(self.input_cursor);
        self.sug_cycle = None;
    }

    pub fn input_home(&mut self) {
        self.input_cursor = 0;
        self.sug_cycle = None;
    }

    pub fn input_end(&mut self) {
        self.input_cursor = self.input_len();
        self.sug_cycle = None;
    }

    fn after_input_change(&mut self) {
        self.sug_cycle = None;
        if matches!(self.mode, Mode::Filter | Mode::Command) {
            self.refresh_suggestions();
        }
    }

    /// Advance the Tab-cycle by `delta` (±1), filling the selected candidate into
    /// the input so Enter applies immediately. The ring is `[typed text] → 0 → 1
    /// → … → N-1 → [typed text]`, so stepping past either end deselects and
    /// restores what you typed. A no-op when there are no candidates.
    pub fn cycle_suggestions(&mut self, delta: isize) {
        match self.sug_cycle.take() {
            None => {
                if self.sug.is_empty() {
                    return;
                }
                let start = self.sug_start;
                let base = self.input[start..].to_string();
                let idx = if delta >= 0 { 0 } else { self.sug.len() - 1 };
                self.input.truncate(start);
                self.input.push_str(&self.sug[idx]);
                self.sug_cycle = Some(Cycle {
                    start,
                    base,
                    cands: self.sug.clone(),
                    idx,
                });
            }
            Some(mut c) => {
                let next = c.idx as isize + delta;
                self.input.truncate(c.start);
                if next < 0 || next >= c.cands.len() as isize {
                    // Stepped past a boundary: deselect and restore the typed
                    // text, leaving the cycle ended (taken above).
                    self.input.push_str(&c.base);
                } else {
                    c.idx = next as usize;
                    self.input.push_str(&c.cands[c.idx]);
                    self.sug_cycle = Some(c);
                }
            }
        }
        // A fill rewrites the tail of the input, so keep the cursor at the end.
        self.input_cursor = self.input.chars().count();
    }

    pub fn suggestions_visible(&self) -> bool {
        matches!(self.mode, Mode::Filter | Mode::Command) && !self.sug.is_empty()
    }

    /// The candidate list to display and the highlighted index (None until Tab
    /// starts a cycle). While cycling, the frozen list is shown so it doesn't
    /// collapse as items are filled.
    pub fn suggestions(&self) -> (&[String], Option<usize>) {
        match &self.sug_cycle {
            Some(c) => (&c.cands, Some(c.idx)),
            None => (&self.sug, None),
        }
    }

    // ----- navigation -------------------------------------------------------

    fn clamp_selection(&mut self) {
        let max = self.view_len().saturating_sub(1);
        self.selected = self.selected.min(max);
    }

    pub fn move_by(&mut self, delta: isize) {
        if self.view_len() == 0 {
            return;
        }
        let new = (self.selected as isize + delta).clamp(0, self.view_len() as isize - 1);
        self.selected = new as usize;
        // Moving away from the newest record stops follow; reaching the end
        // re-enables it.
        self.follow = self.selected + 1 == self.view_len();
        self.detail_scroll = 0;
    }

    pub fn jump_to_top(&mut self) {
        self.selected = 0;
        self.follow = false;
        self.detail_scroll = 0;
    }

    pub fn jump_to_bottom(&mut self) {
        self.selected = self.view_len().saturating_sub(1);
        self.follow = true;
        self.detail_scroll = 0;
    }

    /// Move by a page: the real visible-record count (`whole`) or half of it.
    /// Positive `dir` moves down, negative up.
    pub fn move_page(&mut self, dir: isize, whole: bool) {
        let page = self.page.get().max(1) as isize;
        let step = if whole { page } else { (page / 2).max(1) };
        self.move_by(dir * step);
    }

    pub fn toggle_follow(&mut self) {
        self.follow = !self.follow;
        if self.follow {
            self.jump_to_bottom();
        }
        self.status = if self.follow { "following" } else { "paused" }.into();
    }

    pub fn toggle_raw(&mut self) {
        self.raw = !self.raw;
        self.status = if self.raw { "raw logs" } else { "formatted logs" }.into();
    }

    // ----- selected record --------------------------------------------------

    pub fn selected_record(&self) -> Option<Rc<str>> {
        self.record(self.selected)
    }

    /// One-line rendering of a record for the compact list (parses, redacts,
    /// formats). Newlines the template emits become tabs so each record occupies
    /// exactly one row — otherwise the list windowing (one item = one row)
    /// mis-counts and leaves a stray blank row while scrolling. The renderer
    /// expands the tabs to aligned columns (see `ansi_text`), so the segments
    /// that were separate lines line up in tab-stop columns. In raw mode the
    /// record's own text is shown verbatim (control chars are stripped later).
    pub fn render_row(&self, line: &str) -> String {
        if self.raw {
            return line.replace('\n', "\t");
        }
        self.render_with(line, &self.row_fmt).replace('\n', "\t")
    }

    /// Multi-line rendering of a record for the expanded list — header line plus
    /// pretty data, exactly like piped `jlf`. In raw mode it's the record as
    /// pretty-printed JSON (every field, no recipe), like the detail pane.
    pub fn render_record(&self, line: &str) -> String {
        if self.raw {
            return self.render_detail(line);
        }
        self.render_with(line, &self.full_fmt)
    }

    /// Parse, redact and format `line` with `fmt`, falling back to the raw line
    /// on any error.
    fn render_with(&self, line: &str, fmt: &Formatter) -> String {
        let mut j = Json::Null;
        if j.parse_replace(line).is_err() {
            return line.to_owned();
        }
        if !self.redact.is_empty() {
            jlf_core::redact(&mut j, &self.redact);
        }
        let mut out = String::new();
        if fmt.as_log(&j).write_fmt(&mut out).is_err() {
            return line.to_owned();
        }
        out
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

    /// Iterate the records in the current view, paging the spilled middle back
    /// in as needed. Streams (no full materialization) so summaries and export
    /// stay memory-bounded over very large stores.
    fn view_iter(&self) -> Box<dyn Iterator<Item = Rc<str>> + '_> {
        match &self.view {
            Some(v) => Box::new(v.iter().map(move |&i| self.store.get(i))),
            None => Box::new((0..self.store.len()).map(move |i| self.store.get(i))),
        }
    }

    /// Stream the current view's raw records (one JSON line each, honoring the
    /// active filter) to `path`. Streams via a buffered writer so a huge view
    /// doesn't materialize in memory. Returns the number of records written.
    pub fn write_view_raw(&self, path: &std::path::Path) -> std::io::Result<usize> {
        use std::io::{BufWriter, Write};
        let mut w = BufWriter::new(std::fs::File::create(path)?);
        let mut n = 0;
        for rec in self.view_iter() {
            writeln!(w, "{rec}")?;
            n += 1;
        }
        w.flush()?;
        Ok(n)
    }

    // ----- summaries --------------------------------------------------------

    /// Begin a summary: build the aggregator and fold in a first batch (small
    /// stores finish at once; larger ones continue across frames via
    /// [`Self::tick_summary`]).
    fn start_summary(&mut self, verb: &str, field: Option<&str>, n: usize) {
        match Agg::new(verb, field, n) {
            Ok(agg) => {
                self.summary_job = Some(SummaryJob { agg, cursor: 0 });
                self.tick_summary();
            }
            Err(msg) => self.status = msg,
        }
    }

    /// Fold the next batch of view records into the active summary (and pick up
    /// records that arrived since the last tick), refreshing the rendered panel.
    /// A no-op when no summary is open.
    pub fn tick_summary(&mut self) {
        let Some(mut job) = self.summary_job.take() else {
            return;
        };
        let total = self.view_len();
        let end = (job.cursor + SUMMARY_BATCH).min(total);
        for pos in job.cursor..end {
            if let Some(rec) = self.record(pos) {
                job.agg.feed(&rec);
            }
        }
        job.cursor = end;
        self.summary = Some(job.agg.render(job.cursor, total));
        self.summary_job = Some(job);
    }

    /// Whether a summary is still folding records (so the event loop should keep
    /// ticking promptly instead of idling).
    pub fn summary_computing(&self) -> bool {
        self.summary_job
            .as_ref()
            .is_some_and(|j| j.cursor < self.view_len())
    }

    /// Close the summary panel and drop its aggregator.
    pub fn close_summary(&mut self) {
        self.summary = None;
        self.summary_job = None;
    }

    // ----- commands ---------------------------------------------------------

    pub fn run_command(&mut self, cmd: &str) {
        let mut it = cmd.split_whitespace();
        let Some(verb) = it.next() else { return };
        let rest: Vec<&str> = it.collect();
        match verb {
            "q" | "quit" => self.quit = true,
            "count" => self.start_summary("count", rest.first().copied(), 10),
            "uniq" => self.start_summary("uniq", rest.first().copied(), 10),
            "stats" => self.start_summary("stats", rest.first().copied(), 10),
            "top" => {
                let n = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
                self.start_summary("top", rest.first().copied(), n);
            }
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
        let mut rows = 0usize;
        for line in self.view_iter() {
            let mut j = Json::Null;
            if j.parse_replace(&line).is_err() {
                continue;
            }
            let cells: Vec<String> = cols
                .iter()
                .map(|c| scalar(resolve(&j, c)).unwrap_or("").to_owned())
                .collect();
            write_row(&mut buf, &cells, sep, md);
            rows += 1;
        }
        match std::fs::write(&out_path, buf) {
            Ok(()) => self.status = format!("wrote {rows} rows to {out_path}"),
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
    fn input_cursor_edits_and_motion() {
        let mut app = app_with(SAMPLE);
        app.enter_search();
        for c in "abc def".chars() {
            app.input_char(c);
        }
        assert_eq!(app.input, "abc def");
        assert_eq!(app.input_cursor, 7);

        // Home, then insert at the start.
        app.input_home();
        assert_eq!(app.input_cursor, 0);
        app.input_char('X');
        assert_eq!(app.input, "Xabc def");
        assert_eq!(app.input_cursor, 1);

        // End, delete-word-back removes "def".
        app.input_end();
        app.input_delete_word();
        assert_eq!(app.input, "Xabc ");
        assert_eq!(app.input_cursor, 5);

        // Word-left from the end lands at the start of "Xabc".
        app.input_word_left();
        assert_eq!(app.input_cursor, 0);

        // Delete-forward at the start removes 'X'.
        app.input_delete_forward();
        assert_eq!(app.input, "abc ");
        assert_eq!(app.input_cursor, 0);

        // Ctrl-K style: delete to end from the middle.
        app.input_right();
        app.input_delete_to_end();
        assert_eq!(app.input, "a");
    }

    #[test]
    fn ingests_all_records_into_view() {
        let app = app_with(SAMPLE);
        assert_eq!(app.total(), 3);
        assert_eq!(app.view_len(), 3);
    }

    #[test]
    fn raw_and_write_view_use_the_original_records() {
        let mut app = app_with(SAMPLE);
        // Raw compact rendering is the record verbatim (not the recipe output).
        app.raw = true;
        app.expanded = false;
        assert_eq!(app.render_row(SAMPLE[0]), SAMPLE[0]);
        // Raw expanded rendering is pretty-printed JSON (multi-line).
        let pretty = app.render_record(SAMPLE[0]);
        assert!(pretty.contains('\n') && pretty.contains("\"user\""), "got:\n{pretty}");

        // write_view_raw streams the current view's raw lines, honoring filters.
        let dir = std::env::temp_dir();
        let path = dir.join(format!("jlf-tui-test-{}.jsonl", std::process::id()));
        let n = app.write_view_raw(&path).unwrap();
        assert_eq!(n, 3);
        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(body.lines().count(), 3);
        assert_eq!(body.lines().next().unwrap(), SAMPLE[0]);

        app.apply_filter("level=error".into());
        let n = app.write_view_raw(&path).unwrap();
        assert_eq!(n, 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), SAMPLE[1]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn filter_narrows_the_view() {
        let mut app = app_with(SAMPLE);
        app.apply_filter("level=error,warn".into());
        assert_eq!(app.view_len(), 2);
        app.apply_filter(String::new());
        assert_eq!(app.view_len(), 3);
    }

    #[test]
    fn search_highlights_without_narrowing_and_navigates() {
        let mut app = app_with(SAMPLE);
        app.selected = 0;
        // Search keeps every row (unlike filter) and jumps to the first match.
        app.apply_search("alice".into());
        assert_eq!(app.view_len(), 3, "search must not hide rows");
        assert_eq!(app.selected, 0); // record 0 (alice) already matches
        // n / N walk between matching records (records 0 and 2 have alice).
        app.search_jump(true);
        assert_eq!(app.selected, 2);
        // Position is reported as (current match, total matches).
        assert_eq!(app.search_position(), Some(MatchCount::Counted { current: Some(2), total: 2 }));
        app.search_jump(true); // wraps back to the first match
        assert_eq!(app.selected, 0);
        assert_eq!(app.search_position(), Some(MatchCount::Counted { current: Some(1), total: 2 }));
        app.search_jump(false); // previous wraps forward to the last match
        assert_eq!(app.selected, 2);
        // Off a match, only the total is known.
        app.selected = 1;
        assert_eq!(app.search_position(), Some(MatchCount::Counted { current: None, total: 2 }));
        // Smart-case: a lowercase query matches case-insensitively and spans
        // values (records 0 and 2 contain "alice").
        app.apply_search("bob".into());
        assert_eq!(app.selected, 1);
        assert_eq!(app.search_position(), Some(MatchCount::Counted { current: Some(1), total: 1 }));
        // Clearing removes the search.
        app.apply_search(String::new());
        assert!(app.search_query.is_empty());
        assert_eq!(app.search_position(), None);
    }

    #[test]
    fn search_is_smart_case() {
        let mut app = app_with(SAMPLE); // values are lowercase (alice/bob/info…)
        let total = |a: &App| match a.search_position() {
            Some(MatchCount::Counted { total, .. }) => total,
            _ => usize::MAX,
        };
        // Lowercase query: case-insensitive. "alice" is in records 0 and 2.
        app.apply_search("alice".into());
        assert_eq!(total(&app), 2);
        // Uppercase input still matches (query is lowercase).
        app.apply_search("ALICE".into());
        // …no: "ALICE" has uppercase, so it's case-sensitive and won't match the
        // lowercase data.
        assert_eq!(total(&app), 0);
        // Mixed/exact case matches only the exact case.
        app.apply_search("Alice".into());
        assert_eq!(total(&app), 0);
        // Lowercase "info" (case-insensitive) matches the one info record.
        app.apply_search("info".into());
        assert_eq!(total(&app), 1);
    }

    #[test]
    fn search_jumps_from_the_end_and_n_goes_up() {
        // records 0 and 2 contain "alice"; start at the bottom (record 2).
        let mut app = app_with(SAMPLE);
        app.selected = 2;
        // Enter lands on the newest match (record 2 already matches → stays).
        app.apply_search("alice".into());
        assert_eq!(app.selected, 2);
        // From a non-matching newest record, Enter jumps up to the last match.
        app.selected = 2;
        app.apply_search("42".into()); // only record 0 has latency 42
        assert_eq!(app.selected, 0);
        // n walks upward (older); N walks downward (newer).
        app.apply_search("alice".into());
        app.selected = 2;
        app.search_jump(false); // n → up
        assert_eq!(app.selected, 0);
        app.search_jump(true); // N → down (wraps to record 2)
        assert_eq!(app.selected, 2);
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
        assert_eq!(app.view_len(), 0);
        send(SAMPLE[1]); // an error arrives after the filter is set
        app.drain_input();
        assert_eq!(app.view_len(), 1);
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
    fn summary_updates_live_as_records_arrive() {
        let (tx, rx) = channel();
        let mut app = App::new(rx).unwrap();
        tx.send(SAMPLE[0].to_string()).unwrap(); // one info
        app.drain_input();
        app.run_command("count");
        assert_eq!(app.summary.as_ref().unwrap().rows[0], "1");
        // A new record arrives; the next tick folds it into the running total.
        tx.send(SAMPLE[1].to_string()).unwrap();
        app.drain_input();
        app.tick_summary();
        assert_eq!(app.summary.as_ref().unwrap().rows[0], "2");
    }

    #[test]
    fn summary_recomputes_on_filter_change() {
        let mut app = app_with(SAMPLE);
        app.run_command("count");
        assert_eq!(app.summary.as_ref().unwrap().rows[0], "3");
        app.apply_filter("level=error".into()); // only one record matches
        assert_eq!(app.summary.as_ref().unwrap().rows[0], "1");
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
        let rec = app.selected_record().unwrap();
        let row = app.render_row(&rec);
        assert!(row.contains("***"));
        assert!(!row.contains("secret"));
    }
}
