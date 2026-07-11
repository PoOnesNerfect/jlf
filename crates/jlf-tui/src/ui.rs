use ansi_to_tui::IntoText;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Mode};

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
        "  /         filter / search  :    command",
        "  ?         this help        q    quit",
        "  Esc       close popup / clear filter        ^L   redraw",
        "",
        "Filter / search  (press /)",
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
    let active = matches!(app.mode, Mode::Search | Mode::Command);
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
            Mode::Search => Line::from(Span::styled(" filter ", Style::default().fg(border))),
            Mode::Command => Line::from(Span::styled(" command ", Style::default().fg(border))),
            // Show the keys as distinct, bracketed tokens so it's clear each is a
            // key to press, not part of the sentence.
            Mode::Normal => {
                let key = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
                let dim = Style::default().fg(Color::DarkGray);
                Line::from(vec![
                    Span::styled(" [", dim),
                    Span::styled("/", key),
                    Span::styled("] filter", dim),
                    Span::styled("  ·  [", dim),
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
        return; // empty framed box
    }
    let input = match app.mode {
        Mode::Search => format!("/{}", app.input),
        Mode::Command => format!(":{}", app.input),
        Mode::Normal => String::new(),
    };
    let prefix = 1u16; // the `/` or `:`
    f.render_widget(Paragraph::new(Line::from(input)), inner);
    // Place a real terminal cursor at the edit position so it's clear where
    // typing and deletion will happen.
    let cursor_x = (inner.x + prefix + app.input_cursor as u16)
        .min(inner.x + inner.width.saturating_sub(1));
    f.set_cursor_position((cursor_x, inner.y));
}

/// The always-visible key hint shown on the prompt line in Normal mode.
const HINT: &str = "↑↓ move · g/G top/bottom · d/u page · ⏎ detail · c expand · a actions · ? help · q quit";

/// The bottom bar: always shows the status (follow, position, filter, transient
/// message). Its trailing hint section shows the normal key hints, or — while
/// typing a `/` filter or `:` command — that mode's completion help, so those
/// don't need a separate row (the input itself lives in the section above, see
/// [`draw_input_section`]).
fn draw_bar(f: &mut Frame, app: &App, area: Rect) {
    let follow = if app.follow { "● follow" } else { "‖ paused" };
    let position = if app.view_len() == 0 {
        "0/0".to_string()
    } else {
        format!("{}/{}", app.selected + 1, app.view_len())
    };
    let filter = if app.filter_text.is_empty() {
        "no filter".to_string()
    } else {
        // Show how many records matched out of the total held.
        format!("/{}  ({} of {})", app.filter_text, app.view_len(), app.total())
    };
    let mut spans = vec![
        Span::styled(" jlf-tui ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(format!("  {follow}   {position} records   {filter}")),
    ];
    // Transient feedback (filter cleared, N match, errors, saved…).
    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("   ·   {}", app.status),
            Style::default().fg(Color::Yellow),
        ));
    }
    // The hint section: the completion help while typing, else the key hints.
    let hints = match app.mode {
        Mode::Search | Mode::Command => "Tab/↑↓ cycle · ⏎ apply · Esc cancel",
        Mode::Normal => HINT,
    };
    spans.push(Span::styled(
        format!("    {hints}"),
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Fraction of the viewport kept between the cursor and the top/bottom edge
/// before scrolling kicks in (a "scrolloff" margin, ~30%).
fn scroll_margin(inner_h: usize) -> usize {
    (inner_h * 3 / 10).min(inner_h.saturating_sub(1) / 2)
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" records ");
    let inner_h = area.height.saturating_sub(2) as usize;
    let total = app.view_len();
    let width = area.width as usize;

    if total == 0 {
        f.render_widget(block, area);
        return;
    }
    if app.expanded {
        draw_list_expanded(f, app, area, block, inner_h, total);
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

    let items: Vec<ListItem> = (top..end)
        .filter_map(|pos| app.record(pos))
        .map(|rec| {
            let line = ansi_line(app.render_row(&rec));
            ListItem::new(truncate_line(line, width))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected - top));

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );
    f.render_stateful_widget(list, area, &mut state);
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
    let bar = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

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
                let mut spans = vec![gutter()];
                spans.extend(truncate_line(ansi_line(raw.to_owned()), inner_w.saturating_sub(2)).spans);
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
    let mut top = app.scroll_top.get().min(app.selected);
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
        // Near the buffer end the content underfills the viewport: pull earlier
        // records in until it overflows, then bottom-anchor (clip the top record)
        // so the last record sits flush at the bottom with no empty rows.
        let mut start = top;
        while start > 0 && window_fits(start - 1) {
            start -= 1;
        }
        // one more record (if any) so the panel fills, then keep the last screenful
        start = start.saturating_sub(1);
        render_top = start;
        let mut full = build_from(start);
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

/// Parse an ANSI string into styled ratatui text; fall back to plain on error.
/// Control characters left in the *content* (a stray `\r`, `\t`, backspace… from
/// the log itself) are stripped: `into_text` has already turned real ANSI escape
/// sequences into styles, so anything control-like remaining is literal data
/// that would otherwise move the terminal cursor and corrupt the frame (e.g. a
/// `\r` jumps to column 0 and overwrites the border — which ratatui never
/// repaints, so the damage sticks).
fn ansi_text(s: String) -> Text<'static> {
    let mut text = s.clone().into_text().unwrap_or_else(|_| Text::from(s));
    for line in &mut text.lines {
        for span in &mut line.spans {
            if span.content.chars().any(char::is_control) {
                let cleaned: String = span
                    .content
                    .chars()
                    .filter_map(|c| match c {
                        '\t' => Some(' '),
                        c if c.is_control() => None,
                        c => Some(c),
                    })
                    .collect();
                span.content = cleaned.into();
            }
        }
    }
    text
}

/// Parse an ANSI string known to be a single line into one styled `Line`.
fn ansi_line(s: String) -> Line<'static> {
    ansi_text(s).lines.into_iter().next().unwrap_or_default()
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

    fn buffer_text(lines: &[&str]) -> String {
        let (tx, rx) = channel();
        for l in lines {
            tx.send((*l).to_string()).unwrap();
        }
        drop(tx);
        let mut app = App::new(rx).unwrap();
        app.drain_input();

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
        assert!(text.contains("records"), "missing list title:\n{text}");
        assert!(text.contains("detail"), "missing detail title:\n{text}");
        assert!(text.contains("hello world"), "missing record:\n{text}");
    }

    #[test]
    fn detail_pane_pretty_prints_selected() {
        let text = buffer_text(&[r#"{"a":{"b":1}}"#]);
        // pretty JSON puts the nested key on its own indented line
        assert!(text.contains("\"b\""), "detail not pretty:\n{text}");
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
