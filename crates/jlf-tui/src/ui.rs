use ansi_to_tui::IntoText;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Mode};

pub fn draw(f: &mut Frame, app: &App) {
    let show_sug = app.suggestions_visible();
    let mut constraints = vec![Constraint::Length(1), Constraint::Min(0)];
    if show_sug {
        constraints.push(Constraint::Length(2));
    }
    constraints.push(Constraint::Length(1));
    let areas = Layout::vertical(constraints).split(f.area());
    let (status, main, prompt) = (areas[0], areas[1], areas[areas.len() - 1]);

    draw_status(f, app, status);

    if app.show_detail {
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(main);
        draw_list(f, app, list_area);
        draw_detail(f, app, detail_area);
    } else {
        draw_list(f, app, main);
    }
    if show_sug {
        draw_suggestions(f, app, areas[2]);
    }
    draw_prompt(f, app, prompt);

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
        "  g/G       top / bottom     J/K  scroll detail",
        "  a         actions panel    f    follow on / off",
        "  /         filter / search  :    command",
        "  ?         this help        q    quit",
        "  Esc       close popup / clear filter",
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

fn draw_suggestions(f: &mut Frame, app: &App, area: Rect) {
    let (cands, sel) = app.suggestions();
    let mut spans = vec![Span::styled(" ⇥ ", Style::default().fg(Color::DarkGray))];
    for (i, c) in cands.iter().enumerate() {
        let style = if i == sel {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(format!(" {c} "), style));
        spans.push(Span::raw(" "));
    }
    // Candidates on the first row, the key hint on its own row below.
    let text = vec![
        Line::from(spans),
        Line::from(Span::styled(
            "   Tab/↑↓ pick · ⏎ fill · Esc dismiss",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(Paragraph::new(text), area);
}

/// The always-visible key hint shown on the prompt line in Normal mode.
const HINT: &str = "↑↓ move · ⏎ detail · a actions · / search · : command · ? help · q quit";

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let follow = if app.follow { "● follow" } else { "‖ paused" };
    let position = if app.view.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", app.selected + 1, app.view.len())
    };
    let filter = if app.filter_text.is_empty() {
        "no filter".to_string()
    } else {
        format!("/{}", app.filter_text)
    };
    let mut spans = vec![
        Span::styled(" jlf-tui ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(format!("  {follow}   {position} records   {filter}")),
    ];
    // Transient feedback (filter cleared, N match, errors, saved…) rides in the
    // status bar so it never hides the key hints on the prompt line.
    if !app.status.is_empty() {
        spans.push(Span::styled(
            format!("   ·   {}", app.status),
            Style::default().fg(Color::Yellow),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" records ");
    let inner_h = area.height.saturating_sub(2) as usize;
    let total = app.view.len();

    // Window: render only the visible slice so a million-line buffer doesn't
    // build a million widgets per frame. Keep the cursor on screen.
    let start = app
        .selected
        .saturating_sub(inner_h.saturating_sub(1))
        .min(total.saturating_sub(inner_h));
    let start = if total <= inner_h { 0 } else { start };
    let end = (start + inner_h).min(total);

    let items: Vec<ListItem> = app.view[start..end]
        .iter()
        .map(|&i| {
            let line = ansi_line(app.render_row(&app.lines[i]));
            ListItem::new(truncate_line(line, area.width as usize))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected.saturating_sub(start)));

    let list = List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::Cyan)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD),
    );
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let text = app
        .selected_line()
        .map(|l| ansi_text(app.render_detail(l)))
        .unwrap_or_else(|| Text::from("no record selected"));
    let para = Paragraph::new(text)
        .block(Block::bordered().title(" detail "))
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(para, area);
}

/// Parse an ANSI string into styled ratatui text; fall back to plain on error.
fn ansi_text(s: String) -> Text<'static> {
    s.clone().into_text().unwrap_or_else(|_| Text::from(s))
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

fn draw_prompt(f: &mut Frame, app: &App, area: Rect) {
    let line = match app.mode {
        Mode::Search => Line::from(format!("/{}", app.input)),
        Mode::Command => Line::from(format!(":{}", app.input)),
        // Always show the key hints here so they're never hidden by a message.
        Mode::Normal => Line::from(Span::styled(HINT, Style::default().fg(Color::DarkGray))),
    };
    f.render_widget(Paragraph::new(line), area);
}

fn draw_summary(f: &mut Frame, summary: &crate::summary::Summary, area: Rect) {
    let width = summary
        .rows
        .iter()
        .map(|r| r.len())
        .max()
        .unwrap_or(10)
        .max(summary.title.len())
        + 4;
    let height = summary.rows.len() + 2;
    let popup = center(area, width as u16, height as u16);

    let text: Vec<Line> = summary.rows.iter().map(|r| Line::from(r.as_str())).collect();
    let block = Block::bordered()
        .title(format!(" {} ", summary.title))
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
}
