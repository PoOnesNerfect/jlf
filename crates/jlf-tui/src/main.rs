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
    // Transient status messages ("search cleared", "N match", …) fade a few
    // seconds after they appear so they don't linger. Tracked here (not in App)
    // since it's purely a display concern; a new/changed message resets the clock.
    const STATUS_TTL: Duration = Duration::from_secs(2);
    let mut status_since: Option<std::time::Instant> = None;
    let mut last_status = String::new();
    loop {
        let (_, more_input) = app.drain_input();
        // Advance a running summary (folds a batch of records per frame, and
        // picks up newly-arrived ones) before drawing.
        app.tick_summary();
        // Expire a transient status after STATUS_TTL; reset the clock whenever
        // the message changes.
        if app.status != last_status {
            last_status = app.status.clone();
            status_since = (!app.status.is_empty()).then(std::time::Instant::now);
        } else if status_since.is_some_and(|t| t.elapsed() >= STATUS_TTL) {
            app.status.clear();
            last_status.clear();
            status_since = None;
        }
        // A forced repaint (Ctrl-L) clears any externally-corrupted cells that
        // ratatui's diff would otherwise leave untouched.
        if std::mem::take(&mut app.force_redraw) {
            terminal.clear()?;
        }
        terminal.draw(|f| ui::draw(f, app))?;

        // Poll for input. During a big-file burst there's more buffered input to
        // ingest, so don't block — loop immediately to keep filling and
        // repainting (the count climbs as a progress cue). While a summary folds,
        // poll briefly so it finishes fast without busy-spinning. Otherwise idle.
        let timeout = if more_input {
            Duration::from_millis(0)
        } else if app.summary_computing() {
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
        if std::mem::take(&mut app.pending_editor) {
            open_view_in_editor(app, terminal)?;
        }
        if app.quit {
            return Ok(());
        }
    }
}

/// Write the current (filtered) view to a temp file as raw JSON lines and open it
/// in the user's editor (`$VISUAL`/`$EDITOR`, else a platform default). The TUI
/// owns the terminal, so it's suspended (leave the alternate screen, disable raw
/// mode) around the editor and restored afterward with a forced repaint. All
/// failures are surfaced as a status message rather than crashing the viewer.
fn open_view_in_editor(app: &mut App, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
    use ratatui::crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::crossterm::execute;

    let mut path = std::env::temp_dir();
    path.push(format!("jlf-tui-view-{}.jsonl", std::process::id()));

    let count = match app.write_view_raw(&path) {
        Ok(n) => n,
        Err(e) => {
            app.status = format!("couldn't write temp file: {e}");
            return Ok(());
        }
    };

    // Suspend the TUI, run the editor attached to the terminal, then resume.
    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    // Logs read newest-last, so open at the last line (the newest record).
    let (editor, args) = editor_command(&path, count);
    let status = std::process::Command::new(&editor).args(&args).status();
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen)?;
    terminal.clear()?; // repaint from scratch — the editor overwrote the screen

    let _ = std::fs::remove_file(&path);
    app.status = match status {
        Ok(s) if s.success() => format!("opened {count} records in {editor}"),
        Ok(_) => format!("{editor} exited with an error"),
        Err(e) => format!("couldn't launch {editor}: {e}"),
    };
    Ok(())
}

/// Build the editor command: `$VISUAL`, then `$EDITOR`, then a platform default,
/// plus the file to open positioned at `last_line` (the newest record) when the
/// editor's line-jump syntax is known. Extra words in the env var (e.g.
/// `code --wait`) are kept as leading arguments. Returns `(command, args)` where
/// `args` already includes the file path. For an unrecognized editor the file is
/// opened without positioning — a wrong flag could be taken as a filename, so we
/// only add one for editors we know.
fn editor_command(path: &std::path::Path, last_line: usize) -> (String, Vec<String>) {
    let spec = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| if cfg!(windows) { "notepad".into() } else { "vi".into() });
    build_editor_args(&spec, &path.to_string_lossy(), last_line)
}

/// Pure editor-argument builder (no env), so it's testable. See [`editor_command`].
fn build_editor_args(spec: &str, file: &str, last_line: usize) -> (String, Vec<String>) {
    let mut parts = spec.split_whitespace().map(str::to_owned);
    let cmd = parts.next().unwrap_or_else(|| "vi".into());
    let mut args: Vec<String> = parts.collect();
    let file = file.to_owned();

    // The editor's base name (no path, no `.exe`), lowercased, to pick its
    // line-jump syntax.
    let base = std::path::Path::new(&cmd)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    match base.as_str() {
        // `+LINE file` — vi/vim family, nano, emacs, kakoune.
        "vi" | "vim" | "nvim" | "view" | "gvim" | "mvim" | "nano" | "emacs" | "emacsclient"
        | "kak"
            if last_line > 0 =>
        {
            args.push(format!("+{last_line}"));
            args.push(file);
        }
        // `file:LINE` — helix, sublime.
        "hx" | "helix" | "subl" | "sublime_text" if last_line > 0 => {
            args.push(format!("{file}:{last_line}"));
        }
        // `--goto file:LINE` — VS Code and friends.
        "code" | "code-insiders" | "codium" | "vscodium" | "cursor" | "windsurf"
            if last_line > 0 =>
        {
            args.push("--goto".into());
            args.push(format!("{file}:{last_line}"));
        }
        // Unknown editor (or empty view): just open the file at the start.
        _ => args.push(file),
    }
    (cmd, args)
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
        Mode::Filter | Mode::Search | Mode::Command => handle_input(app, code, mods),
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
        KeyCode::Char('r') => app.toggle_raw(),
        // `e` opens the current (filtered) view in $EDITOR. It needs to suspend
        // the terminal, which only the run loop can do, so just flag it here.
        KeyCode::Char('e') => app.pending_editor = true,
        KeyCode::Char('a') => app.open_actions(),
        KeyCode::Char('h') => app.help = !app.help,
        KeyCode::Char('/') => app.enter_search(),
        KeyCode::Char('?') => app.enter_filter(),
        KeyCode::Char(':') => app.enter_command(),
        // n / N step between search matches. Logs read newest-last, so `n` walks
        // upward (toward older records) and `N` downward (toward newer), matching
        // the "newest first" direction Enter jumps to.
        KeyCode::Char('n') => app.search_jump(false),
        KeyCode::Char('N') => app.search_jump(true),
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
            } else if !app.search_query.is_empty() {
                app.apply_search(String::new());
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
            // deletes the `/`/`:` prefix — exiting the mode (and clearing the
            // filter, since deleting the whole thing means "no filter").
            KeyCode::Char('w') => {
                if app.input.is_empty() {
                    exit_search_or_command(app);
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
                Mode::Filter => app.apply_filter(text),
                Mode::Search => app.apply_search(text),
                Mode::Command => app.run_command(&text),
                Mode::Normal => {}
            }
            app.mode = Mode::Normal;
        }
        KeyCode::Esc => {
            // Cancel search: drop the incremental preview and return to the
            // anchor (no-op in filter/command mode).
            if matches!(app.mode, Mode::Search) {
                app.cancel_search();
            }
            app.mode = Mode::Normal;
            app.input.clear();
        }
        KeyCode::Left => app.input_left(),
        KeyCode::Right => app.input_right(),
        KeyCode::Home => app.input_home(),
        KeyCode::End => app.input_end(),
        KeyCode::Delete => app.input_delete_forward(),
        // Backspace deletes the char before the cursor; on an empty input it
        // deletes the `?`/`/`/`:` prefix — i.e. exits the mode.
        KeyCode::Backspace => input_backspace_or_exit(app),
        KeyCode::Char(c) => app.input_char(c),
        _ => {}
    }
}

/// Backspace when there's text, else delete the visual prefix — which leaves the
/// field. Deleting the whole `?` filter or `/` search this way clears it (empty
/// means none), rather than keeping the previously-applied one.
fn input_backspace_or_exit(app: &mut App) {
    if app.input.is_empty() {
        exit_search_or_command(app);
    } else {
        app.input_backspace();
    }
}

/// Leave the input field. Deleting out of an applied `?` filter or `/` search
/// clears it so the view/highlight resets. (Esc, by contrast, cancels the edit
/// and keeps whatever was already applied.)
fn exit_search_or_command(app: &mut App) {
    match app.mode {
        Mode::Filter if !app.filter_text.is_empty() => app.apply_filter(String::new()),
        // Deleting out of search: clear an applied query, else just cancel the
        // preview back to the anchor.
        Mode::Search if !app.search_query.is_empty() => app.apply_search(String::new()),
        Mode::Search => app.cancel_search(),
        _ => {}
    }
    app.mode = Mode::Normal;
}

#[cfg(test)]
mod tests {
    use super::build_editor_args;

    #[test]
    fn editor_args_open_at_last_line() {
        // vi/vim family, nano, emacs, kakoune → `+LINE file`
        assert_eq!(
            build_editor_args("vim", "/tmp/v.jsonl", 42),
            ("vim".into(), vec!["+42".into(), "/tmp/v.jsonl".into()])
        );
        assert_eq!(
            build_editor_args("nano", "/tmp/v.jsonl", 5),
            ("nano".into(), vec!["+5".into(), "/tmp/v.jsonl".into()])
        );
        // helix / sublime → `file:LINE`
        assert_eq!(
            build_editor_args("hx", "/tmp/v.jsonl", 7),
            ("hx".into(), vec!["/tmp/v.jsonl:7".into()])
        );
        // VS Code → `--goto file:LINE`, keeping extra env args (e.g. --wait)
        assert_eq!(
            build_editor_args("code --wait", "/tmp/v.jsonl", 9),
            (
                "code".into(),
                vec!["--wait".into(), "--goto".into(), "/tmp/v.jsonl:9".into()]
            )
        );
        // A full path to the editor still resolves by base name.
        assert_eq!(
            build_editor_args("/usr/bin/nvim", "/tmp/v.jsonl", 3),
            ("/usr/bin/nvim".into(), vec!["+3".into(), "/tmp/v.jsonl".into()])
        );
        // Unknown editor → just the file (no risky positioning flag).
        assert_eq!(
            build_editor_args("myedit", "/tmp/v.jsonl", 3),
            ("myedit".into(), vec!["/tmp/v.jsonl".into()])
        );
        // Empty view (line 0) → no positioning even for a known editor.
        assert_eq!(
            build_editor_args("vim", "/tmp/v.jsonl", 0),
            ("vim".into(), vec!["/tmp/v.jsonl".into()])
        );
    }
}
