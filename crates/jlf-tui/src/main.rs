mod app;
mod catalog;
mod field;
mod reader;
mod save;
mod summary;
mod ui;

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

    // Keyboard events reach us through fd 0. When something other than a
    // terminal is on stdin (a piped data stream, a redirected file), crossterm
    // can't read keys from it — on macOS it fails to even initialize its reader.
    // So point fd 0 at the controlling terminal, saving the piped data (when we
    // need it as the source) on a fresh fd for the reader.
    let want_stdin_data = file.is_none();
    let piped = match prepare_terminal_input(want_stdin_data) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("jlf-tui: no terminal available for keyboard input.");
            eprintln!(
                "        run it attached to a terminal, e.g. `jlf tui app.log` or `cat logs | jlf tui`."
            );
            std::process::exit(1);
        }
    };
    let source = match file {
        Some(f) => Some(Source::File(f)),
        None => piped,
    };
    let rx = match source {
        Some(s) => reader::spawn(s, true),
        None => channel().1,
    };

    let mut app = App::new(rx)?;
    if !filters.is_empty() {
        app.apply_filter(filters.join(" "));
    }

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

/// Ensure fd 0 is a real terminal so crossterm can read key events there.
///
/// If stdin is already a terminal, nothing to do. Otherwise reopen the
/// controlling terminal onto fd 0; when `want_data` is set (stdin is our data
/// source) the incoming pipe is first duplicated to a new fd and returned as a
/// [`Source::Pipe`] for the reader. Fails only when there is no controlling
/// terminal at all (a truly headless run).
///
/// We reopen the terminal by its real device path (`/dev/ttysNNN`, discovered
/// from stdout/stderr via `ttyname`) rather than `/dev/tty`: on macOS the
/// `/dev/tty` alias device can't be registered with `kqueue`, so crossterm's
/// event reader fails to initialize on it, while the real pts device works.
#[cfg(unix)]
fn prepare_terminal_input(want_data: bool) -> std::io::Result<Option<Source>> {
    use std::io::IsTerminal;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    if std::io::stdin().is_terminal() {
        return Ok(None);
    }
    // Save the piped data on a new fd before fd 0 is repurposed.
    let data = if want_data {
        let fd = unsafe { libc::dup(libc::STDIN_FILENO) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Some(Source::Pipe(unsafe { std::fs::File::from_raw_fd(fd) }))
    } else {
        None
    };
    // Put the controlling terminal on fd 0 for crossterm's event reader. Prefer
    // the real device path over `/dev/tty` (see the doc comment).
    let path = terminal_device_path().unwrap_or_else(|| "/dev/tty".into());
    let tty = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    if unsafe { libc::dup2(tty.as_raw_fd(), libc::STDIN_FILENO) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(data)
}

/// The real device path of the terminal attached to stdout, stderr, or stdin
/// (whichever is a tty). Returns `None` when none of them is a terminal.
#[cfg(unix)]
fn terminal_device_path() -> Option<std::path::PathBuf> {
    for fd in [libc::STDOUT_FILENO, libc::STDERR_FILENO, libc::STDIN_FILENO] {
        if unsafe { libc::isatty(fd) } != 1 {
            continue;
        }
        let name = unsafe { libc::ttyname(fd) };
        if !name.is_null() {
            let s = unsafe { std::ffi::CStr::from_ptr(name) };
            if let Ok(s) = s.to_str() {
                return Some(std::path::PathBuf::from(s));
            }
        }
    }
    None
}

/// Non-Unix fallback: read piped stdin directly (crossterm reads the console
/// separately from a stdin pipe on Windows).
#[cfg(not(unix))]
fn prepare_terminal_input(want_data: bool) -> std::io::Result<Option<Source>> {
    use std::io::IsTerminal;
    if want_data && !std::io::stdin().is_terminal() {
        Ok(Some(Source::Stdin))
    } else {
        Ok(None)
    }
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
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    // Ctrl-C always quits.
    if ctrl && matches!(code, KeyCode::Char('c')) {
        app.quit = true;
        return;
    }
    match app.mode {
        Mode::Normal => {
            if ctrl {
                match code {
                    KeyCode::Char('d') => app.move_by(HALF_PAGE),
                    KeyCode::Char('u') => app.move_by(-HALF_PAGE),
                    _ => {}
                }
            } else {
                handle_normal(app, code);
            }
        }
        Mode::Search | Mode::Command => handle_input(app, code, ctrl),
    }
}

fn handle_normal(app: &mut App, code: KeyCode) {
    // The Actions panel captures keys while it's open.
    if app.show_actions {
        match code {
            KeyCode::Esc | KeyCode::Char('a') => app.show_actions = false,
            KeyCode::Char('j') | KeyCode::Down => app.action_move(1),
            KeyCode::Char('k') | KeyCode::Up => app.action_move(-1),
            KeyCode::Enter => app.run_action(),
            _ => {}
        }
        return;
    }

    // A help or summary popup intercepts dismiss keys first.
    if (app.help || app.summary.is_some()) && matches!(code, KeyCode::Esc | KeyCode::Char('q')) {
        app.help = false;
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
        KeyCode::Char('a') => app.open_actions(),
        KeyCode::Char('?') => app.help = !app.help,
        KeyCode::Char('/') => app.enter_search(),
        KeyCode::Char(':') => app.enter_command(),
        // Enter opens/closes the detail pane for the selected record.
        KeyCode::Enter => {
            app.show_detail = !app.show_detail;
            app.detail_scroll = 0;
        }
        KeyCode::Esc => {
            if app.help {
                app.help = false;
            } else if app.summary.is_some() {
                app.summary = None;
            } else if app.show_detail {
                app.show_detail = false;
            } else if !app.filter_text.is_empty() {
                app.apply_filter(String::new());
            }
        }
        _ => {}
    }
}

fn handle_input(app: &mut App, code: KeyCode, ctrl: bool) {
    // Search autocomplete: Tab/Shift-Tab and ↑/↓ move the highlight; Enter fills
    // the highlighted suggestion, or commits the filter when none is showing;
    // Esc dismisses the popup, then cancels. Command mode ignores suggestions.
    if ctrl {
        match code {
            KeyCode::Char('w') => app.input_delete_word(),
            KeyCode::Char('n') => app.suggestion_move(1),
            KeyCode::Char('p') => app.suggestion_move(-1),
            _ => {}
        }
        return;
    }
    match code {
        KeyCode::Tab | KeyCode::Down if app.suggestions_visible() => app.suggestion_move(1),
        KeyCode::BackTab | KeyCode::Up if app.suggestions_visible() => app.suggestion_move(-1),
        KeyCode::Enter => {
            if app.suggestions_visible() && app.fill_suggestion() {
                return;
            }
            let text = std::mem::take(&mut app.input);
            match app.mode {
                Mode::Search => app.apply_filter(text),
                Mode::Command => app.run_command(&text),
                Mode::Normal => {}
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Esc => {
            if app.suggestions_visible() {
                app.dismiss_suggestions();
            } else {
                app.mode = Mode::Normal;
                app.input.clear();
            }
        }
        KeyCode::Backspace => app.input_backspace(),
        KeyCode::Char(c) => app.input_char(c),
        _ => {}
    }
}
