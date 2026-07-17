use ansi_to_tui::IntoText;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Clear, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Wrap,
};
use ratatui::Frame;

use crate::app::{App, MatchCount, Mode};

pub fn draw(f: &mut Frame, app: &App) {
    // Fixed layout: the list (+ optional detail), then a framed input box, then
    // the single bottom bar. The box is always present (so nothing shifts) and
    // holds the `/` filter or `:` command input with its candidates; when idle
    // it's an empty, titled frame so the space reads as a deliberate input area.
    let areas = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
        Constraint::Length(1),
    ])
    .split(f.area());
    let main = areas[0];
    let sug = areas[1];
    let bar = areas[2];

    if app.show_detail {
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(main);
        draw_list(f, app, list_area);
        draw_detail(f, app, detail_area);
    } else {
        draw_list(f, app, main);
    }
    draw_input_section(f, app, sug);
    draw_bar(f, app, bar);

    if app.help {
        draw_help(f, main);
    } else if app.show_actions {
        draw_actions(f, app, main);
    } else if let Some(summary) = &app.summary {
        draw_summary(f, summary, main);
    }
}

fn draw_actions(f: &mut Frame, app: &App, area: Rect) {
    use crate::app::ACTIONS;
    let width = ACTIONS.iter().map(|(l, _)| l.len()).max().unwrap_or(20) as u16 + 8;
    let height = ACTIONS.len() as u16 + 2;
    let popup = center(area, width, height);
    let items: Vec<ListItem> = ACTIONS
        .iter()
        .map(|(label, _)| ListItem::new(Line::from(format!("  {label}"))))
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.action_sel));
    let list = List::new(items)
        .block(
            Block::bordered()
                .title(" actions ")
                .title_bottom(" ↑↓ move · ⏎ run · esc close "),
        )
        .style(Style::default().bg(Color::Black))
        .highlight_style(
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("❯ ");
    f.render_widget(Clear, popup);
    f.render_stateful_widget(list, popup, &mut state);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let rows = [
        "Keys",
        "  ↑/k ↓/j   move            ⏎    detail on / off",
        "  d/u       half page down / up   D/U  full page",
        "  g/G       top / bottom     J/K  jump 7 / scroll detail",
        "  a         actions panel    f    follow on / off",
        "  c         compact / expand rows",
        "  r         raw / formatted rows    e    open view in $EDITOR",
        "  /         search           n/N  match up / down",
        "  ?         filter           :    command",
        "  h         this help        q    quit",
        "  Esc       close popup / clear search / clear filter    ^L  redraw",
        "",
        "Search  (press /)  — highlights matches, keeps every row",
        "  matches text anywhere in a record (key or value); n/N step up / down",
        "",
        "Filter  (press ?)  — narrows to matching rows",
        "  field=value   op: = != > >= < <= ~ !~   (e.g. level=error)",
        "  bare words    match anywhere in the record (e.g. timeout)",
        "",
        "Actions (a) & commands (:)",
        "  count [field]          stats <field>",
        "  top <field> [n]        uniq <field>",
        "  redact <globs>         follow",
        "  csv|tsv|md <cols> [file]   save <name>",
        "  help                   quit",
    ];
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(20) as u16 + 4;
    let height = rows.len() as u16 + 2;
    let popup = center(area, width, height);
    let text: Vec<Line> = rows
        .iter()
        .map(|r| {
            if !r.is_empty() && !r.starts_with(' ') {
                Line::from(Span::styled(
                    *r,
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(*r)
            }
        })
        .collect();
    let block = Block::bordered()
        .title(" help ")
        .title_bottom(" esc / q to close ");
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(text)
            .block(block)
            .style(Style::default().bg(Color::Black)),
        popup,
    );
}

/// The framed input box. Always drawn (so the layout never shifts) and only
/// three rows tall: the top border doubles as the row for suggestion candidates
/// while typing (or the box title when idle / no candidates), the single inner
/// row holds the `/` filter or `:` command input, and the bottom border closes
/// it. Idle it's an empty titled frame, so the reserved space reads as a
/// deliberate input area rather than blank rows.
fn draw_input_section(f: &mut Frame, app: &App, area: Rect) {
    let active = matches!(app.mode, Mode::Filter | Mode::Search | Mode::Command);
    let border = if active { Color::Cyan } else { Color::DarkGray };

    // The top border row: the candidates while cycling, else a label.
    let title = if app.suggestions_visible() {
        let (cands, sel) = app.suggestions();
        let mut spans = vec![Span::styled(" ⇥ ", Style::default().fg(Color::DarkGray))];
        for (i, c) in cands.iter().enumerate() {
            let style = if Some(i) == sel {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(format!(" {c} "), style));
            spans.push(Span::raw(" "));
        }
        Line::from(spans)
    } else {
        match app.mode {
            Mode::Search => Line::from(Span::styled(" search ", Style::default().fg(border))),
            Mode::Filter => Line::from(Span::styled(" filter ", Style::default().fg(border))),
            Mode::Command => Line::from(Span::styled(" command ", Style::default().fg(border))),
            // Show the keys as distinct, bracketed tokens so it's clear each is a
            // key to press, not part of the sentence.
            Mode::Normal => {
                let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
                let dim = Style::default().fg(Color::DarkGray);
                let sep = || Span::styled("  ·  [", dim);
                Line::from(vec![
                    Span::styled(" [", dim),
                    Span::styled("/", key),
                    Span::styled("] search", dim),
                    sep(),
                    Span::styled("?", key),
                    Span::styled("] filter", dim),
                    sep(),
                    Span::styled(":", key),
                    Span::styled("] command ", dim),
                ])
            }
        }
    };

    let block = Block::bordered()
        .border_style(Style::default().fg(border))
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if !active {
        draw_status_line(f, app, inner);
        return;
    }
    let prefix = mode_prefix(&app.mode);
    let mut spans = vec![Span::raw(format!("{prefix}{}", app.input))];
    // While searching, show the live match count right next to the query so
    // it's obvious at the point of typing whether it hits.
    if matches!(app.mode, Mode::Search) && !app.input.is_empty() {
        if let Some(label) = search_count_label(app) {
            spans.push(Span::styled(
                format!("   {label}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }
    let cursor_col = prefix.len_utf8() + app.input_cursor;
    f.render_widget(Paragraph::new(Line::from(spans)), inner);
    // Place a real terminal cursor at the edit position so it's clear where
    // typing and deletion will happen.
    let cursor_x = (inner.x + cursor_col as u16)
        .min(inner.x + inner.width.saturating_sub(1));
    f.set_cursor_position((cursor_x, inner.y));
}

/// The live match-count label shown during search (`3 matches`, `no matches`,
/// or `matches` when the view is too large to count). Shared by the input row
/// and the bottom bar so they never disagree.
fn search_count_label(app: &App) -> Option<String> {
    match app.search_position()? {
        MatchCount::Counted { total: 0, .. } => Some("no matches".to_string()),
        MatchCount::Counted { total: 1, .. } => Some("1 match".to_string()),
        MatchCount::Counted { total, .. } => Some(format!("{total} matches")),
        MatchCount::Partial { found: 0 } => Some("? matches".to_string()),
        MatchCount::Partial { found } => Some(format!("{found}+ matches")),
    }
}

/// The `?`/`/`/`:` prefix character shown before an active input.
fn mode_prefix(mode: &Mode) -> char {
    match mode {
        Mode::Search => '/',
        Mode::Filter => '?',
        Mode::Command => ':',
        Mode::Normal => ' ',
    }
}

/// The idle input row: the view's status line. The active search (`/…`) comes
/// first, so it stays where you typed it in the input rather than jumping across
/// the row; then the active filter (`?…`) or "No filter", then the record count.
/// This is the single place both are shown, so the bottom bar doesn't repeat them.
fn draw_status_line(f: &mut Frame, app: &App, inner: Rect) {
    let dim = Style::default().fg(Color::DarkGray);
    let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::raw(" ")];
    if !app.search_query.is_empty() {
        spans.push(Span::styled("/", key));
        spans.push(Span::styled(
            app.search_query.clone(),
            Style::default().fg(Color::Black).bg(SEARCH_HL),
        ));
        // The current match position out of the total, so `n`/`N` progress is
        // visible. Just the total when the selection isn't on a match; nothing
        // when the view is too large to count cheaply.
        let label = match app.search_position() {
            Some(MatchCount::Counted { current: Some(i), total }) => {
                Some(format!("   {i}/{total} matches"))
            }
            Some(MatchCount::Counted { current: None, total }) => {
                Some(format!("   {total} matches"))
            }
            Some(MatchCount::Partial { found: 0 }) => Some("   ? matches".to_string()),
            Some(MatchCount::Partial { found }) => Some(format!("   {found}+ matches")),
            _ => None,
        };
        if let Some(label) = label {
            spans.push(Span::styled(label, dim));
        }
        spans.push(Span::styled("   ·   ", dim));
    }
    if app.filter_text.is_empty() {
        spans.push(Span::styled("No filter", dim));
    } else {
        spans.push(Span::styled("?", key));
        spans.push(Span::styled(app.filter_text.clone(), Style::default().fg(Color::Cyan)));
    }
    spans.push(Span::styled(
        format!("   ·   {}/{} records", app.view_len(), app.total()),
        dim,
    ));
    if app.raw {
        spans.push(Span::styled(
            "   ·   raw",
            Style::default().fg(Color::Yellow),
        ));
    }
    f.render_widget(
        Paragraph::new(truncate_line(Line::from(spans), inner.width as usize)),
        inner,
    );
}

/// The highlight color for search matches (in the list and the status line).
const SEARCH_HL: Color = Color::Yellow;

fn record_count_title(total: usize) -> String {
    if total == 1 {
        " 1 record ".to_string()
    } else {
        format!(" {total} records ")
    }
}

fn cursor_line_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    }
}

fn cursor_gutter_style(focused: bool) -> Style {
    let style = Style::default().fg(if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    });
    if focused {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

/// The always-visible key hint shown on the prompt line in Normal mode.
const HINT: &str = "↑↓ move · / search · ? filter · c expand · r raw · e editor · a actions · h help · q quit";

/// The bottom bar: the app badge, follow state, and a transient message, then
/// the key-hint section (completion help while typing a `/` filter or `:`
/// command, else the normal keys). The active filter and record count live in
/// the input box above (see [`draw_input_section`]), so they aren't repeated here.
fn draw_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![Span::styled(
        " jlf-tui ",
        Style::default().fg(Color::Black).bg(Color::Cyan),
    )];
    // Follow state: a green dot while auto-scrolling to the newest record, a red
    // bar when paused.
    if app.follow {
        spans.push(Span::styled("  ● ", Style::default().fg(Color::Green)));
        spans.push(Span::raw("follow"));
    } else {
        spans.push(Span::styled("  ‖ ", Style::default().fg(Color::Red)));
        spans.push(Span::raw("paused"));
    }
    // Transient feedback (filter cleared, N match, errors, saved…).
    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("   ·   {}", app.status),
            Style::default().fg(Color::Yellow),
        ));
    }
    // The hint section: completion help while filtering/commanding, a search
    // hint (with the live match count) while searching, else the normal keys.
    match app.mode {
        Mode::Search => {
            // The count now sits next to the input; the bar just shows actions.
            spans.push(Span::styled(
                "    ⏎ jump · Esc cancel".to_string(),
                Style::default().fg(Color::DarkGray),
            ));
        }
        _ => {
            let hints = match app.mode {
                Mode::Filter | Mode::Command => "Tab/↑↓ cycle · ⏎ apply · Esc cancel",
                _ => HINT,
            };
            spans.push(Span::styled(
                format!("    {hints}"),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Fraction of the viewport kept between the cursor and the top/bottom edge
/// before scrolling kicks in (a "scrolloff" margin, ~30%).
fn scroll_margin(inner_h: usize) -> usize {
    (inner_h * 3 / 10).min(inner_h.saturating_sub(1) / 2)
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(record_count_title(app.total()));
    let inner_h = area.height.saturating_sub(2) as usize;
    let total = app.view_len();
    let width = area.width as usize;

    if total == 0 {
        f.render_widget(block, area);
        return;
    }
    if app.expanded {
        draw_list_expanded(f, app, area, block, inner_h, total);
        draw_scrollbar(f, app, area, total);
        return;
    }

    // Compact: one record per row. Scroll with a margin so the cursor moves
    // freely inside the viewport and only scrolls near the edges; window to the
    // visible slice so a million-line buffer doesn't build a million widgets.
    let margin = scroll_margin(inner_h);
    let max_top = total.saturating_sub(inner_h);
    let mut top = app.scroll_top.get().min(max_top);
    if app.selected < top + margin {
        top = app.selected.saturating_sub(margin);
    } else if app.selected + 1 + margin > top + inner_h {
        top = (app.selected + 1 + margin).saturating_sub(inner_h);
    }
    top = top.min(max_top);
    app.scroll_top.set(top);
    let end = (top + inner_h).min(total);
    app.page.set(end - top);
    // Prefetch a screenful beyond each edge so scrolling into the spilled middle
    // stays smooth.
    app.prefetch(top.saturating_sub(1));
    app.prefetch(end);

    let needle = app.search_needle();
    let items: Vec<ListItem> = (top..end)
        .filter_map(|pos| app.record(pos))
        .map(|rec| {
            let line = highlight_line(ansi_line(app.render_row(&rec)), needle);
            ListItem::new(truncate_line(line, width))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected - top));

    let list = List::new(items)
        .block(block)
        .highlight_style(cursor_line_style(app.focused));
    f.render_stateful_widget(list, area, &mut state);
    draw_scrollbar(f, app, area, total);
}

/// A vertical scrollbar on the list's right border, marking the selected
/// record's position in the whole stream. Drawn only when the records don't all
/// fit (otherwise there's nothing to scroll).
fn draw_scrollbar(f: &mut Frame, app: &App, area: Rect, total: usize) {
    let page = app.page.get().max(1);
    if total <= page {
        return;
    }
    let mut state = ScrollbarState::new(total)
        .viewport_content_length(page)
        .position(app.selected);
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(None)
        .thumb_symbol("█")
        .thumb_style(Style::default().fg(Color::Cyan));
    // Span the full box height (including the border rows) so the thumb reaches
    // the very top/bottom frame when the selection is at either end — otherwise
    // it stops a row short and reads as "not quite at the edge". With no track
    // symbol, only the thumb is painted, so the border and its corners show
    // through everywhere the thumb isn't.
    f.render_stateful_widget(bar, area, &mut state);

    // Cap the thumb where it lands on a frame corner with a half block — lower
    // half at the top, upper half at the bottom — so it meets the corner edge
    // instead of covering it with a full block hanging past the frame.
    let x = area.right().saturating_sub(1);
    for (y, cap) in [(area.top(), "▄"), (area.bottom().saturating_sub(1), "▀")] {
        if let Some(cell) = f.buffer_mut().cell_mut((x, y)) {
            if cell.symbol() == "█" {
                cell.set_symbol(cap);
            }
        }
    }
}

/// Expanded list (toggled with `c`): each record spans multiple lines — header
/// plus pretty data with a blank line between records, exactly like piped `jlf`.
/// Records have variable heights, so this composes them into a flat line buffer
/// and clips at line granularity like a pager. The viewport top is a record
/// index scrolled with the same margin as compact mode, so the cursor moves
/// freely and only scrolls near the edges; at the buffer's end the content fills
/// from the bottom. The selected record keeps its own colors, marked by a left
/// bar (a full-block highlight would bury the syntax coloring).
fn draw_list_expanded(
    f: &mut Frame,
    app: &App,
    area: Rect,
    block_widget: Block,
    inner_h: usize,
    total: usize,
) {
    use std::cell::RefCell;
    use std::collections::HashMap;

    let inner_w = area.width.saturating_sub(2) as usize;
    let bar = cursor_gutter_style(app.focused);
    let needle = app.search_needle();

    use std::rc::Rc;
    // A record's content lines plus whether its template asked for a trailing
    // blank separator; see the memoizing `block` closure below.
    type Block = Rc<(Vec<Line<'static>>, bool)>;

    // A record's guttered, width-clipped content lines, plus whether its template
    // asked for a trailing blank separator (it ends in a newline). Memoized
    // behind `Rc` so the scroll math's repeated length checks don't deep-clone.
    // The separator is tracked here but emitted only *between* records, never
    // after the last visible one, so the bottom row is never a stray blank.
    let cache: RefCell<HashMap<usize, Block>> = RefCell::new(HashMap::new());
    let block = |i: usize| -> Block {
        if let Some(v) = cache.borrow().get(&i) {
            return v.clone();
        }
        let selected = i == app.selected;
        let gutter = || Span::styled(if selected { "▌ " } else { "  " }, bar);
        let record = app.record(i).unwrap_or_else(|| Rc::from(""));
        let rendered = app.render_record(&record);
        let had_sep = rendered.ends_with('\n');
        let body = rendered.strip_suffix('\n').unwrap_or(&rendered);
        let lines: Vec<Line> = body
            .split('\n')
            .map(|raw| {
                let content = highlight_line(ansi_line(raw.to_owned()), needle);
                let mut spans = vec![gutter()];
                spans.extend(truncate_line(content, inner_w.saturating_sub(2)).spans);
                Line::from(spans)
            })
            .collect();
        let v = Rc::new((lines, had_sep));
        cache.borrow_mut().insert(i, v.clone());
        v
    };
    // A record's effective height including the separator that follows it.
    let height = |i: usize| -> usize {
        let b = block(i);
        b.0.len() + b.1 as usize
    };

    let margin = scroll_margin(inner_h);
    let sel_h = height(app.selected);

    // The topmost record when scrolled fully to the bottom, so content fills from
    // the bottom and the newest record can sit at the very bottom edge (no forced
    // margin below it) rather than leaving a gap at the end of the buffer.
    let mut rows = 0usize;
    let mut max_top = total - 1;
    for i in (0..total).rev() {
        rows += height(i);
        max_top = i;
        if rows >= inner_h {
            break;
        }
    }

    // Rows above the selected block for a given viewport top.
    let above = |t: usize| -> usize { (t..app.selected).map(height).sum() };

    // Sticky scrolloff: start from the persisted top and only move it when the
    // cursor would leave the margin band, so moving within the viewport doesn't
    // scroll. Each pass is one-directional (terminates); order matters.
    //
    // Clamp the starting `top` to at most `inner_h` records back from the
    // selection: since every record is ≥1 row, no record earlier than that can
    // be on screen, and this bounds the `above()` height sums to a viewport's
    // worth of records. Without it, the first frame after a jump-to-bottom (or a
    // `G` from the top) sums heights from 0 to the selection — rendering the
    // whole buffer just to measure it (seconds on a large stream).
    let floor = app.selected.saturating_sub(inner_h);
    let mut top = app.scroll_top.get().clamp(floor, app.selected);
    // 1) Keep the selected block's bottom on screen (scroll down if it overflows;
    //    a block taller than the viewport shows from its own top).
    while top < app.selected && above(top) + sel_h > inner_h {
        top += 1;
    }
    // 2) Top margin: if the cursor is within `margin` rows of the top, scroll up.
    while top > 0 && above(top) < margin {
        top -= 1;
    }
    // 3) Bottom margin: scroll down to keep `margin` below the cursor — but only
    //    while there's more content below to reveal (`top < max_top`). At the
    //    buffer end this is a no-op, so the newest record rests at the bottom and
    //    moving up walks the cursor through the viewport before it scrolls.
    while top < max_top && above(top) + sel_h + margin > inner_h {
        top += 1;
    }

    // Build the visible lines from a start index, inserting a blank separator
    // between records that asked for one (never a trailing one).
    let build_from = |start: usize| -> Vec<Line<'static>> {
        let mut out: Vec<Line> = Vec::new();
        for i in start..total {
            if i > start && block(i - 1).1 {
                out.push(Line::from(""));
            }
            out.extend(block(i).0.iter().cloned());
        }
        out
    };
    // Whether every record from `from` to the end fits within the viewport (kept
    // cheap by bailing out as soon as it overflows).
    let window_fits = |from: usize| -> bool {
        let mut r = 0usize;
        for i in from..total {
            r += block(i).0.len();
            if i + 1 < total && block(i).1 {
                r += 1;
            }
            if r > inner_h {
                return false;
            }
        }
        true
    };

    // The topmost record actually rendered — persisted so next frame's scrolloff
    // math matches what's on screen (otherwise the fill/anchor and the sticky top
    // diverge and the cursor gets pinned).
    let render_top;
    let mut lines = if window_fits(top) {
        // Near the buffer end the content underfills the viewport. `start` is the
        // first record whose window (start..end) still fits; that's the first
        // fully-visible record, so persist it as the top (keeping the scrolloff
        // math consistent with the screen — otherwise the persisted top sits a
        // record above what's shown and the next keypress scrolls spuriously).
        let mut start = top;
        while start > 0 && window_fits(start - 1) {
            start -= 1;
        }
        render_top = start;
        // Build from one record earlier so the panel fills, then bottom-anchor
        // (clip that earlier record's top) so the last record is flush at the
        // bottom with no empty rows.
        let mut full = build_from(start.saturating_sub(1));
        if full.len() > inner_h {
            full = full.split_off(full.len() - inner_h);
        }
        full
    } else {
        // Scrolled up: emit from `top` and clip the bottom to the viewport.
        render_top = top;
        let mut out: Vec<Line> = Vec::new();
        for i in top..total {
            if i > top && block(i - 1).1 {
                out.push(Line::from(""));
            }
            out.extend(block(i).0.iter().cloned());
            if out.len() >= inner_h {
                break;
            }
        }
        out.truncate(inner_h);
        out
    };
    lines.truncate(inner_h);
    app.scroll_top.set(render_top);

    // Page size (for d/u/D/U): records that fit from the rendered top.
    let mut page = 0usize;
    let mut h = 0usize;
    for i in render_top..total {
        h += height(i);
        page += 1;
        if h >= inner_h {
            break;
        }
    }
    app.page.set(page.max(1));

    f.render_widget(Paragraph::new(lines).block(block_widget), area);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let text = app
        .selected_record()
        .map(|l| ansi_text(app.render_detail(&l)))
        .unwrap_or_else(|| Text::from("no record selected"));
    let para = Paragraph::new(text)
        .block(Block::bordered().title(" detail "))
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(para, area);
}

/// Standard tab-stop width used when expanding tabs in rendered content.
const TAB_STOP: usize = 8;

/// Parse an ANSI string into styled ratatui text; fall back to plain on error.
/// Control characters left in the *content* (a stray `\r`, backspace… from the
/// log itself) are stripped, since `into_text` has already turned real ANSI
/// escape sequences into styles, so anything control-like remaining is literal
/// data that would otherwise move the terminal cursor and corrupt the frame
/// (e.g. a `\r` jumps to column 0 and overwrites the border — which ratatui
/// never repaints, so the damage sticks). Tabs are expanded to spaces up to the
/// next tab stop (ratatui doesn't lay out tabs), tracked across spans so the
/// columns stay aligned — this is how compact rows lay out the segments that
/// were separate template lines.
fn ansi_text(s: String) -> Text<'static> {
    let mut text = s.clone().into_text().unwrap_or_else(|_| Text::from(s));
    for line in &mut text.lines {
        let mut col = 0usize;
        for span in &mut line.spans {
            if span.content.chars().any(|c| c.is_control()) {
                span.content = expand_controls(&span.content, &mut col).into();
            } else {
                col += span.content.chars().count();
            }
        }
    }
    text
}

/// Strip corrupting control chars and expand tabs to the next [`TAB_STOP`],
/// advancing `col` (the running display column of the line so tabs from later
/// spans still land on tab stops).
fn expand_controls(content: &str, col: &mut usize) -> String {
    let mut out = String::new();
    for c in content.chars() {
        match c {
            '\t' => {
                let pad = TAB_STOP - (*col % TAB_STOP);
                out.push_str(&" ".repeat(pad));
                *col += pad;
            }
            c if c.is_control() => {}
            c => {
                out.push(c);
                *col += 1;
            }
        }
    }
    out
}

/// Parse an ANSI string known to be a single line into one styled `Line`.
fn ansi_line(s: String) -> Line<'static> {
    ansi_text(s).lines.into_iter().next().unwrap_or_default()
}

/// Highlight occurrences of `needle` in a styled line by splitting spans at match
/// boundaries and overlaying the search style on the matched characters (keeping
/// their surrounding colors). Smart-case: matches case-insensitively when the
/// needle is all lowercase, case-sensitively when it has any uppercase — the same
/// rule search navigation uses, so highlights and `n`/`N` agree. Case-folding
/// keeps a 1:1 char count so match positions line up with the original spans.
fn highlight_line(line: Line<'static>, needle: &str) -> Line<'static> {
    if needle.is_empty() {
        return line;
    }
    let case_sensitive = crate::app::search_case_sensitive(needle);
    let fold = |c: char| if case_sensitive { c } else { c.to_ascii_lowercase() };
    let needle: Vec<char> = needle.chars().map(fold).collect();
    let lower: Vec<char> = line
        .spans
        .iter()
        .flat_map(|s| s.content.chars())
        .map(fold)
        .collect();
    if lower.len() < needle.len() {
        return line;
    }
    // Mark each char position that falls inside a (non-overlapping) match.
    let mut matched = vec![false; lower.len()];
    let mut found = false;
    let mut i = 0;
    while i + needle.len() <= lower.len() {
        if lower[i..i + needle.len()] == needle[..] {
            matched[i..i + needle.len()].fill(true);
            found = true;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    if !found {
        return line;
    }
    let hl = Style::default().fg(Color::Black).bg(SEARCH_HL);
    // Rebuild spans, breaking each where the matched flag flips.
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut pos = 0;
    for span in line.spans {
        let mut seg = String::new();
        let mut seg_matched = false;
        for c in span.content.chars() {
            if !seg.is_empty() && matched[pos] != seg_matched {
                let style = if seg_matched { span.style.patch(hl) } else { span.style };
                out.push(Span::styled(std::mem::take(&mut seg), style));
            }
            seg_matched = matched[pos];
            seg.push(c);
            pos += 1;
        }
        if !seg.is_empty() {
            let style = if seg_matched { span.style.patch(hl) } else { span.style };
            out.push(Span::styled(seg, style));
        }
    }
    Line::from(out)
}

/// Clip a styled line to `max` columns, appending an ellipsis (preserving the
/// per-span styles/colors so truncation keeps the record colored).
fn truncate_line(line: Line<'static>, max: usize) -> Line<'static> {
    let max = max.saturating_sub(2);
    let mut used = 0;
    let mut spans: Vec<Span<'static>> = Vec::new();
    for span in line.spans {
        let w = span.content.chars().count();
        if used + w <= max {
            used += w;
            spans.push(span);
        } else {
            let take = max.saturating_sub(used).saturating_sub(1);
            let mut s: String = span.content.chars().take(take).collect();
            s.push('…');
            spans.push(Span::styled(s, span.style));
            break;
        }
    }
    Line::from(spans)
}

fn draw_summary(f: &mut Frame, summary: &crate::summary::Summary, area: Rect) {
    // Size to the content but keep a comfortable minimum so the panel reads as a
    // deliberate result rather than a tiny box lost in the middle.
    let content_w = summary
        .rows
        .iter()
        .map(|r| r.chars().count())
        .max()
        .unwrap_or(0)
        .max(summary.title.len());
    let width = (content_w + 6).clamp(40, area.width.saturating_sub(4) as usize);
    let height = (summary.rows.len() + 4).clamp(7, area.height.saturating_sub(2) as usize);
    let popup = center(area, width as u16, height as u16);

    // A blank line above the rows gives the numbers room to breathe.
    let mut text: Vec<Line> = vec![Line::from("")];
    text.extend(summary.rows.iter().map(|r| Line::from(format!("  {r}"))));
    let block = Block::bordered()
        .border_style(Style::default().fg(Color::Cyan))
        .title(Line::from(Span::styled(
            format!(" {} ", summary.title),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )))
        .title_bottom(" esc to close ");
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(text)
            .block(block)
            .style(Style::default().bg(Color::Black)),
        popup,
    );
}

fn center(area: Rect, width: u16, height: u16) -> Rect {
    let [h] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [v] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(h);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::sync::mpsc::channel;

    #[test]
    fn highlight_splits_spans_on_matches() {
        // "abcABCxyz" searching "abc" (case-insensitive) matches the first two
        // runs; being adjacent they merge into one highlighted span, then "xyz".
        let line = Line::from(vec![Span::raw("abcABCxyz")]);
        let out = highlight_line(line, "abc");
        let texts: Vec<String> = out.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(texts, vec!["abcABC", "xyz"]);
        let hl = Style::default().fg(Color::Black).bg(SEARCH_HL);
        assert_eq!(out.spans[0].style, hl);
        assert_eq!(out.spans[1].style, Style::default());
    }

    #[test]
    fn highlight_is_smart_case() {
        // A query with uppercase is case-sensitive: only the exact-case run of
        // "ABC" is highlighted, not the lowercase "abc".
        let line = Line::from(vec![Span::raw("abcABCxyz")]);
        let out = highlight_line(line, "ABC");
        let texts: Vec<String> = out.spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(texts, vec!["abc", "ABC", "xyz"]);
        let hl = Style::default().fg(Color::Black).bg(SEARCH_HL);
        assert_eq!(out.spans[0].style, Style::default());
        assert_eq!(out.spans[1].style, hl);
        assert_eq!(out.spans[2].style, Style::default());
    }

    #[test]
    fn highlight_no_match_is_unchanged() {
        let line = Line::from(vec![Span::raw("hello"), Span::raw("world")]);
        let out = highlight_line(line, "zzz");
        assert_eq!(out.spans.len(), 2);
        // An empty needle is also a no-op.
        let line = Line::from(vec![Span::raw("hello")]);
        assert_eq!(highlight_line(line, "").spans.len(), 1);
    }

    fn buffer_text(lines: &[&str]) -> String {
        render_app(lines, |_| {})
    }

    /// Build an app from `lines`, apply `setup` (e.g. toggle detail/expanded),
    /// render one frame, and return the flattened terminal buffer as text.
    fn render_app(lines: &[&str], setup: impl FnOnce(&mut App)) -> String {
        let (tx, rx) = channel();
        for l in lines {
            tx.send((*l).to_string()).unwrap();
        }
        drop(tx);
        let mut app = App::new(rx).unwrap();
        app.drain_input();
        setup(&mut app);

        let mut term = Terminal::new(TestBackend::new(90, 12)).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();
        let buf = term.backend().buffer().clone();
        let area = *buf.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_chrome_and_a_record() {
        let text = buffer_text(&[r#"{"level":"info","msg":"hello world"}"#]);
        assert!(text.contains("jlf-tui"), "missing header:\n{text}");
        assert!(text.contains("1 record"), "missing list title:\n{text}");
        assert!(text.contains("hello world"), "missing record:\n{text}");
    }

    #[test]
    fn cursor_line_grays_out_when_terminal_is_unfocused() {
        assert_eq!(
            cursor_line_style(true),
            Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            cursor_line_style(false),
            Style::default().bg(Color::DarkGray).fg(Color::White)
        );
        assert_eq!(
            cursor_gutter_style(false),
            Style::default().fg(Color::DarkGray)
        );
    }

    #[test]
    fn detail_pane_pretty_prints_selected() {
        // Detail is opt-in (Enter), so open it before rendering.
        let text = render_app(&[r#"{"a":{"b":1}}"#], |app| app.show_detail = true);
        assert!(text.contains("detail"), "missing detail title:\n{text}");
        // pretty JSON puts the nested key on its own indented line
        assert!(text.contains("\"b\""), "detail not pretty:\n{text}");
    }

    #[test]
    fn tabs_expand_to_aligned_columns() {
        // A tab advances to the next multiple of TAB_STOP; `col` carries across
        // spans so later tabs still align. Other control chars are dropped.
        let mut col = 0;
        assert_eq!(expand_controls("ab\tc", &mut col), "ab      c");
        assert_eq!(col, 9);
        // Continuing on the same line, a leading tab pads from the running col.
        assert_eq!(expand_controls("\tx", &mut col), "       x");
        // A stray carriage return is stripped, not rendered.
        let mut c2 = 0;
        assert_eq!(expand_controls("be\rfore", &mut c2), "before");
    }

    #[test]
    fn control_chars_in_content_are_stripped() {
        // A literal carriage-return *byte* in a value used to jump the terminal
        // cursor to column 0 and overwrite the border; it must be removed from
        // the rendered content so it can never reach the terminal. (Use a real
        // CR byte, not a JSON `\r` escape, which the parser keeps as literal
        // text.)
        let json = "{\"level\":\"info\",\"fields\":{\"message\":\"before\rafter\"}}";
        let text = buffer_text(&[json]);
        assert!(text.contains("beforeafter"), "CR not stripped from content:\n{text}");
        assert!(!text.contains('\r'), "raw CR leaked into the frame:\n{text}");
    }

}
