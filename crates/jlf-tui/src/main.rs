mod app;
mod store;
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

/// Records the selection jumps for a Shift-J / Shift-K "fast move".
const JUMP: isize = 7;

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
    // A live pipe (`cmd -f | jlf tui`) has an upstream producer that keeps
    // running after we quit, so the shell blocks on it. Note it so we can stop
    // it on exit.
    let from_pipe = matches!(source, Some(Source::Pipe(_)) | Some(Source::Stdin));
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
    if from_pipe {
        stop_pipeline_producer();
    }
    result
}

/// When our input was a live pipe, terminate the upstream producer on exit so
/// the shell doesn't block waiting on a still-running `… -f`. We signal only our
/// own process group, and only when it is a subordinate pipeline group (its
/// group id differs from the session id) — so the interactive shell, which leads
/// the session, is never signalled.
#[cfg(unix)]
fn stop_pipeline_producer() {
    unsafe {
        if libc::getpgrp() != libc::getsid(0) {
            // Ignore the signal in ourselves (we're already exiting cleanly),
            // then send it to the rest of the group (the producer).
            libc::signal(libc::SIGTERM, libc::SIG_IGN);
            libc::kill(0, libc::SIGTERM);
        }
    }
}

#[cfg(not(unix))]
fn stop_pipeline_producer() {}

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
        // Advance a running summary (folds a batch of records per frame, and
        // picks up newly-arrived ones) before drawing.
        app.tick_summary();
        // A forced repaint (Ctrl-L) clears any externally-corrupted cells that
        // ratatui's diff would otherwise leave untouched.
        if std::mem::take(&mut app.force_redraw) {
            terminal.clear()?;
        }
        terminal.draw(|f| ui::draw(f, app))?;

        // While a summary is still catching up, poll briefly so it finishes
        // quickly; otherwise idle longer to stay cheap.
        let timeout = if app.summary_computing() {
            Duration::from_millis(5)
        } else {
            Duration::from_millis(100)
        };
        if event::poll(timeout)? {
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
    // Ctrl-L forces a full repaint from any mode (recover a corrupted display).
    if ctrl && matches!(code, KeyCode::Char('l')) {
        app.force_redraw = true;
        return;
    }
    match app.mode {
        Mode::Normal => {
            if ctrl {
                match code {
                    KeyCode::Char('d') => app.move_page(1, false),
                    KeyCode::Char('u') => app.move_page(-1, false),
                    _ => {}
                }
            } else {
                handle_normal(app, code);
            }
        }
        Mode::Search | Mode::Command => handle_input(app, code, mods),
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
        app.close_summary();
        return;
    }

    match code {
        KeyCode::Char('q') => app.quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.move_by(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_by(-1),
        KeyCode::Char('g') | KeyCode::Home => app.jump_to_top(),
        KeyCode::Char('G') | KeyCode::End => app.jump_to_bottom(),
        KeyCode::Char('d') => app.move_page(1, false),
        KeyCode::Char('u') => app.move_page(-1, false),
        KeyCode::Char('D') | KeyCode::PageDown => app.move_page(1, true),
        KeyCode::Char('U') | KeyCode::PageUp => app.move_page(-1, true),
        // Shift-J/K: scroll the detail pane while it's open (you're inspecting
        // one record), otherwise fast-move the selection by a few records.
        KeyCode::Char('J') => {
            if app.show_detail {
                app.detail_scroll = app.detail_scroll.saturating_add(1);
            } else {
                app.move_by(JUMP);
            }
        }
        KeyCode::Char('K') => {
            if app.show_detail {
                app.detail_scroll = app.detail_scroll.saturating_sub(1);
            } else {
                app.move_by(-JUMP);
            }
        }
        KeyCode::Char('f') => app.toggle_follow(),
        KeyCode::Char('c') => app.expanded = !app.expanded,
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
                app.close_summary();
            } else if app.show_detail {
                app.show_detail = false;
            } else if !app.filter_text.is_empty() {
                app.apply_filter(String::new());
            }
        }
        _ => {}
    }
}

fn handle_input(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let alt = mods.contains(KeyModifiers::ALT);
    // Autocomplete: Tab/Shift-Tab (or ↑/↓, Ctrl-n/p) select and *fill* successive
    // candidates so Enter applies immediately; nothing is selected until the
    // first Tab. Enter commits what's shown; Esc exits the field. The cursor can
    // move anywhere in the input (arrows, word jumps, Home/End) and editing acts
    // at that position, mirroring a terminal readline.
    if ctrl {
        match code {
            // Ctrl-W deletes the word before the cursor; on an empty input it
            // deletes the `/`/`:` prefix — i.e. exits the mode.
            KeyCode::Char('w') => {
                if app.input.is_empty() {
                    app.mode = Mode::Normal;
                } else {
                    app.input_delete_word();
                }
            }
            KeyCode::Char('u') => app.input_delete_to_start(),
            KeyCode::Char('k') => app.input_delete_to_end(),
            KeyCode::Char('d') => app.input_delete_forward(),
            KeyCode::Char('a') => app.input_home(),
            KeyCode::Char('e') => app.input_end(),
            KeyCode::Char('b') | KeyCode::Left => app.input_word_left(),
            KeyCode::Char('f') | KeyCode::Right => app.input_word_right(),
            KeyCode::Char('n') => app.cycle_suggestions(1),
            KeyCode::Char('p') => app.cycle_suggestions(-1),
            KeyCode::Char('h') => input_backspace_or_exit(app),
            _ => {}
        }
        return;
    }
    // Alt-Left/Right and Alt-b/f jump by word.
    if alt {
        match code {
            KeyCode::Left | KeyCode::Char('b') => app.input_word_left(),
            KeyCode::Right | KeyCode::Char('f') => app.input_word_right(),
            _ => {}
        }
        return;
    }
    match code {
        KeyCode::Tab | KeyCode::Down if app.suggestions_visible() => app.cycle_suggestions(1),
        KeyCode::BackTab | KeyCode::Up if app.suggestions_visible() => app.cycle_suggestions(-1),
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input);
            match app.mode {
                Mode::Search => app.apply_filter(text),
                Mode::Command => app.run_command(&text),
                Mode::Normal => {}
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input.clear();
        }
        KeyCode::Left => app.input_left(),
        KeyCode::Right => app.input_right(),
        KeyCode::Home => app.input_home(),
        KeyCode::End => app.input_end(),
        KeyCode::Delete => app.input_delete_forward(),
        // Backspace deletes the char before the cursor; on an empty input it
        // deletes the `/`/`:` prefix — i.e. exits the mode.
        KeyCode::Backspace => input_backspace_or_exit(app),
        KeyCode::Char(c) => app.input_char(c),
        _ => {}
    }
}

/// Backspace when there's text, or exit the search/command mode when the input
/// is already empty (the visual `/`/`:` prefix is what you'd delete next).
fn input_backspace_or_exit(app: &mut App) {
    if app.input.is_empty() {
        app.mode = Mode::Normal;
    } else {
        app.input_backspace();
    }
}
