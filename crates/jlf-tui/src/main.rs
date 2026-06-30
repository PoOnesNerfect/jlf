mod app;
mod field;
mod reader;
mod summary;
mod ui;

use std::io::IsTerminal;
use std::sync::mpsc::channel;
use std::time::Duration;

use jlf_core::Filter;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use app::{App, Mode};
use reader::Source;

/// Selection moved by a fixed half-page for Ctrl-d/Ctrl-u (the exact viewport
/// height isn't known to the app, and a fixed jump is predictable enough).
const HALF_PAGE: isize = 15;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    // Classify args: `key=value` tokens are initial filters, anything else is a
    // file to read.
    let mut file = None;
    let mut filters = Vec::new();
    for a in std::env::args().skip(1) {
        if Filter::parse(&a).is_some() {
            filters.push(a);
        } else {
            file = Some(a);
        }
    }

    // Keyboard events come from /dev/tty, so a piped stdin is free to be the
    // data source. With no file and no pipe there is simply nothing to read.
    let source = match file {
        Some(f) => Some(Source::File(f)),
        None if !std::io::stdin().is_terminal() => Some(Source::Stdin),
        None => None,
    };
    let rx = match source {
        Some(s) => reader::spawn(s, true),
        None => channel().1,
    };

    let mut app = App::new(rx)?;
    if !filters.is_empty() {
        app.apply_filter(filters.join(" "));
    }

    // crossterm reads keys from stdin when it's a tty, otherwise from /dev/tty.
    // In a fully headless context neither exists, so fail early with a clear
    // message rather than a mid-run backtrace after entering the alt screen.
    if !std::io::stdin().is_terminal() && std::fs::File::open("/dev/tty").is_err() {
        eprintln!("jlf-tui: no terminal available for keyboard input.");
        eprintln!("        run it attached to a terminal, e.g. `jlf tui` or `cat logs | jlf tui`.");
        std::process::exit(1);
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> color_eyre::Result<()> {
    loop {
        app.drain_input();
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code, key.modifiers);
                }
            }
        }
        if app.quit {
            return Ok(());
        }
    }
}

fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    if mods.contains(KeyModifiers::CONTROL) {
        match code {
            KeyCode::Char('c') => app.quit = true,
            KeyCode::Char('d') => app.move_by(HALF_PAGE),
            KeyCode::Char('u') => app.move_by(-HALF_PAGE),
            _ => {}
        }
        return;
    }

    match app.mode {
        Mode::Normal => handle_normal(app, code),
        Mode::Search | Mode::Command => handle_input(app, code),
    }
}

fn handle_normal(app: &mut App, code: KeyCode) {
    // A summary popup intercepts dismiss keys first.
    if app.summary.is_some() && matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
        app.summary = None;
        return;
    }

    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.move_by(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_by(-1),
        KeyCode::Char('g') | KeyCode::Home => app.jump_to_top(),
        KeyCode::Char('G') | KeyCode::End => app.jump_to_bottom(),
        KeyCode::Char('J') | KeyCode::PageDown => app.detail_scroll = app.detail_scroll.saturating_add(1),
        KeyCode::Char('K') | KeyCode::PageUp => app.detail_scroll = app.detail_scroll.saturating_sub(1),
        KeyCode::Char('f') => app.toggle_follow(),
        KeyCode::Char('/') => {
            app.mode = Mode::Search;
            app.input = app.filter_text.clone();
        }
        KeyCode::Char(':') => {
            app.mode = Mode::Command;
            app.input.clear();
        }
        KeyCode::Esc => {
            if app.summary.is_some() {
                app.summary = None;
            } else if !app.filter_text.is_empty() {
                app.apply_filter(String::new());
            }
        }
        _ => {}
    }
}

fn handle_input(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input.clear();
        }
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input);
            match app.mode {
                Mode::Search => app.apply_filter(text),
                Mode::Command => app.run_command(&text),
                Mode::Normal => {}
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(c) => app.input.push(c),
        _ => {}
    }
}
