use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Mode};

pub fn draw(f: &mut Frame, app: &App) {
    let [status, main, prompt] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());

    draw_status(f, app, status);

    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(main);

    draw_list(f, app, list_area);
    draw_detail(f, app, detail_area);
    draw_prompt(f, app, prompt);

    if let Some(summary) = &app.summary {
        draw_summary(f, summary, main);
    }
}

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
    let line = Line::from(vec![
        Span::styled(" jlf-tui ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(format!("  {follow}   {position} records   {filter}")),
    ]);
    f.render_widget(Paragraph::new(line), area);
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
        .map(|&i| ListItem::new(truncate(app.render_row(&app.lines[i]), area.width as usize)))
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
        .map(|l| app.render_detail(l))
        .unwrap_or_else(|| "no record selected".into());
    let para = Paragraph::new(text)
        .block(Block::bordered().title(" detail "))
        .wrap(Wrap { trim: false })
        .scroll((app.detail_scroll, 0));
    f.render_widget(para, area);
}

fn draw_prompt(f: &mut Frame, app: &App, area: Rect) {
    let line = match app.mode {
        Mode::Search => Line::from(format!("/{}", app.input)),
        Mode::Command => Line::from(format!(":{}", app.input)),
        Mode::Normal => Line::from(Span::styled(
            &app.status,
            Style::default().fg(Color::DarkGray),
        )),
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

fn truncate(mut s: String, max: usize) -> String {
    let max = max.saturating_sub(2);
    if s.chars().count() > max {
        s = s.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
    }
    s
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
