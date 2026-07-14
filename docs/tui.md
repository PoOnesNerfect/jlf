# The jlf interactive viewer (`jlf tui`)

`jlf tui` is a full-screen terminal viewer for JSON logs: it live-tails a file or
pipe, renders each record in color, and lets you filter, search, summarize,
redact, and export without leaving the screen. It reuses the same recipes and
`$`-template rendering as the `jlf` CLI, so what you see matches piped `jlf`
output.

It is a *navigator*, not a pager: the default one-line-per-record layout is built
for scanning many records and drilling into one, and it stays responsive on long
streams by keeping memory bounded (see [Memory on long streams](#memory-on-long-streams)).

- [Running it](#running-it)
- [Layout](#layout)
- [Keys](#keys)
- [Compact and expanded views](#compact-and-expanded-views)
- [Raw view and opening in an editor](#raw-view-and-opening-in-an-editor)
- [Scrolling](#scrolling)
- [Search](#search)
- [Filter](#filter)
- [Commands](#commands)
- [The Actions panel](#the-actions-panel)
- [Summaries](#summaries)
- [Export](#export)
- [Redaction](#redaction)
- [Saving a recipe](#saving-a-recipe)
- [Memory on long streams](#memory-on-long-streams)
- [Recovering a corrupted display](#recovering-a-corrupted-display)

## Running it

`jlf tui` and `jlf-tui` are the same program (the former dispatches to the
latter, git-style):

```sh
jlf tui app.log                # open a file (keeps following appends)
tail -f app.log | jlf tui      # follow a live pipe
docker logs -f web | jlf tui   # any live stream
jlf tui app.log level=error    # start with a filter applied
```

Arguments shaped like `field=value` are treated as an initial filter; anything
else is a file to open. With no file argument it reads from stdin.

When you pass a live pipe, `jlf tui` reattaches the keyboard to the terminal so
keys still work, and terminates the upstream producer on exit so the shell prompt
returns cleanly.

## Layout

- **Record list** — the colored records, one dense line each by default.
- **Detail pane** — opened with `Enter`, shows the selected record as
  syntax-highlighted, pretty-printed JSON.
- **Input box** — a framed box below the list. While idle it's the view's status
  line: the active filter with its `?` prefix (or "No filter"), a
  `matched/total records` count, and the active search (`/query`) when set. While
  you're typing a `/` search, `?` filter, or `:` command it holds that input and
  its completion candidates. Commands (`:`) don't persist here — they produce a
  summary popup or a status message, and the box returns to the status line.
- **Bottom bar** — the app badge, the follow state, any transient message, and
  the key hints. While you're typing it swaps in that mode's hints. (The filter,
  search, and record count live in the input box above.)

## Keys

| key | action |
| --- | ------ |
| `j` / `k`, `↓` / `↑` | move selection by one |
| `J` / `K` | jump the selection by 7 (or scroll the detail pane while it's open) |
| `d` / `u` | half page down / up |
| `D` / `U`, `PageDown` / `PageUp` | full page down / up |
| `g` / `G`, `Home` / `End` | jump to top / bottom |
| `Enter` | open / close the detail pane |
| `c` | toggle compact / expanded rows |
| `r` | toggle raw rows (the record as-is instead of the recipe output) |
| `e` | open the current view in `$EDITOR` (raw JSON, honoring the filter) |
| `f` | toggle follow (auto-scroll to the newest record) |
| `a` | open the **Actions** panel |
| `/` | search (highlight matches, keep every row) |
| `n` / `N` | step to the search match above / below (up = older, down = newer) |
| `?` | filter (narrow to matching rows) |
| `:` | command |
| `h` | help overlay |
| `Esc` | close a popup, then clear the search, then the filter |
| `Ctrl-L` | force a full redraw |
| `q`, `Ctrl-C` | quit |

Following turns off automatically when you move up from the newest record, and
back on when you return to the bottom (`G`).

## Compact and expanded views

Press `c` to switch the list between two renderings:

- **Compact** — one dense line per record. Best for scanning: it fits the most
  records on screen at once. The template's line break becomes a tab, so the JSON
  data lands in a tab-stop column after the message.
- **Expanded** — each record over multiple lines, exactly like piped `jlf` (a
  header line plus pretty-printed data). Blank lines between records appear only
  when your `output` recipe's template ends in a newline; the view never inserts
  separators on its own.

Which view opens first follows your config's `compact` flag (`compact = true`
starts compact, otherwise expanded); `c` toggles it either way.

The detail pane (`Enter`) is independent of this toggle and always pretty-prints
the single selected record.

## Raw view and opening in an editor

Press `r` to toggle **raw** rows — the record as it arrived, bypassing the recipe
formatting. It combines with `c`: raw + compact shows the original JSON line,
raw + expanded shows it pretty-printed (every field, no recipe). A `raw` marker
appears in the status line while it's on.

Press `e` to open the current view in your editor (`$VISUAL`, then `$EDITOR`,
else `vi`/`notepad`). The records are written to a temp file as raw JSON lines,
one per record, **honoring the active filter** — so `?level=error` then `e` opens
just the error records. The viewer suspends while the editor runs and resumes
when it exits; the temp file is removed afterward (edits aren't saved back).

## Scrolling

Both views keep a scrolloff margin: the cursor moves freely inside the viewport
and the list only scrolls once the cursor comes within roughly 30% of the top or
bottom edge. At the very bottom the newest record sits at the bottom edge, so
moving up walks the cursor through the visible records before the view scrolls.
A record taller than the viewport is shown from its top.

A scrollbar on the list's right edge marks the selected record's position in the
whole stream. It appears only when the records don't all fit on screen.

## Search

Press `/` to search. Search **highlights** matching text without hiding any
rows — every record stays visible, and the matched text is shown reversed in
yellow. Matching looks anywhere in the record (any key or value) and is
**smart-case**: an all-lowercase query matches case-insensitively, while a query
with any uppercase matches exactly (which is also the faster path). Type to
highlight incrementally; **Enter** jumps to the newest match (the last one, at or
above the selection — logs read newest-last, so it lands on the most recent hit),
and then **`n`** steps **up** (toward older records) and **`N`** steps **down**
(toward newer), wrapping around. The active query and your position among the
matches are shown in the status line (`/query 3/12 matches`). Clear it with
**Esc**, or by deleting the whole query and the `/` prefix.

`n`/`N` work at any scale (the jump is an incremental scan). The `k/total` count
is shown only while the view is small enough to scan cheaply (~100k records); on
a larger view the count is omitted rather than shown partially, but navigation
still works.

Search and filter are independent and compose: a search highlights within the
current (possibly filtered) view.

## Filter

Press `?` to filter — this **narrows** the view to matching records. The input
mixes two kinds of token:

- **Structured filters** — `field op value`, with operators `=`, `!=`, `>`,
  `>=`, `<`, `<=`, `~` (contains), `!~` (does not contain). Example:
  `?level=error status>=500`. Numeric operators (and `stats`) read a leading
  number from the value, so a unit-suffixed field like `"6.193 ms"` compares and
  aggregates as `6.193`. Non-numeric values fall back to timestamp comparison
  across the common log formats (ISO 8601 / RFC 3339, RFC 2822, Apache
  common-log, log4j, month-name, syslog), so `?ts>2026-07-11T15:00:00Z` (or a
  partial bound like `?ts>2026-07-11`) filters by time.
- **Bare words** — any token that isn't a `field op value` matches anywhere in
  the raw record text, case-insensitively. Example: `?timeout`.

You can combine them: `?level=error timeout` keeps error records that also
mention `timeout`. Multiple structured filters are ANDed.

The input autocompletes field paths (including nested and array paths like
`fields.status` or `spans.0.method`), then operators, then that field's observed
values. Nothing is selected until you press **Tab** / **↓**, which selects and
fills successive candidates so **Enter** applies immediately; **Shift-Tab** /
**↑** steps back and, past the first item, restores what you typed. **Esc**
exits the input; editing the text deselects.

While typing a `/` search, `?` filter, or `:` command you can move and edit
anywhere in the line with the usual terminal keys: **←/→** move by character,
**Ctrl-←/→** or **Alt-←/→** (also **Alt-B/F**) move by word, **Home**/**Ctrl-A**
and **End**/**Ctrl-E** jump to the start/end. **Backspace** and **Delete** remove
the character before/after the cursor, **Ctrl-W** deletes the word before it,
**Ctrl-U** clears to the start, and **Ctrl-K** clears to the end. Backspace (or
Ctrl-W) on an empty input deletes the `?`/`/`/`:` prefix and leaves the field —
and for a `?` filter or `/` search that also clears it, so deleting the whole
thing means "none". (**Esc** cancels instead, keeping whatever was already
applied.)

Filters and searches apply across the whole stream, including records that have
spilled to disk — not just what's currently in memory.

## Commands

Press `:` for a command. Commands also autocomplete (the verb, then a field).

| command | effect |
| --- | ------ |
| `count [field]` | total records, or a frequency breakdown of a field |
| `stats <field>` | count / min / max / mean / p50 / p90 / p99 of a numeric field |
| `top <field> [n]` | the most frequent values of a field, with their share |
| `uniq <field>` | number of distinct values of a field |
| `redact <globs>` | mask matching fields in the display (comma-separated) |
| `csv\|tsv\|md <cols> [file]` | export the current view as a table |
| `follow` | toggle follow |
| `save <name>` | save the current filter and redaction as a recipe |
| `help` | show the help overlay |
| `quit` | quit |

Summaries and exports run against the **current filtered view**.

## The Actions panel

Press `a` for a menu of the common operations without typing a command: a plain
count runs immediately; the rest (count-by-field, stats, top, uniq, the three
export formats, and save-as-recipe) drop you into the command line pre-filled, so
you just type the field, columns, or name with autocomplete.

## Summaries

`count` / `stats` / `top` / `uniq` compute over the **entire** stream — memory
and the spilled file — not just the records currently in RAM. A large store is
folded incrementally across frames, so the panel shows a `computing…` progress
line instead of freezing, and it keeps updating live as new records arrive.
Close the panel with `Esc` or `q`.

## Export

`csv`, `tsv`, and `md` write the current view as a table:

```
:csv timestamp,level,message           # -> jlf-export.csv
:md  ts,level,latency_ms report.md      # explicit file
```

The first argument is a comma-separated list of columns (field paths); the
optional second argument is the output file (it defaults to `jlf-export.<kind>`).
CSV/TSV values are quoted when needed; Markdown emits a table.

## Redaction

`:redact a,b.c` masks the listed fields in every rendered record (comma-separated
field paths). It only affects the display and exports, never the underlying data.
`:redact` with no argument clears it. Redaction is also saved by `save`.

## Saving a recipe

`:save <name>` writes the current filter and redaction as a `[recipe.<name>]`
entry in your workspace config (`.jlf.toml` / `jlf.toml`), so you can rerun the
same view later with `jlf @<name>` or reopen it in the viewer. See
[DSL.md](DSL.md) for the recipe format.

## Memory on long streams

A viewer that held every line in memory would grow without bound on a busy
`tail -f`. Instead `jlf tui` keeps the **first** and the **most recent** records
resident — the two places `g` and `G` jump to — and spills the middle to a
temporary file, paging chunks back on demand (and prefetching just off the
visible edges so scrolling stays smooth). Memory stays bounded no matter how long
the stream runs; the temp file is removed on exit.

This is transparent in use: navigation, filtering, and summaries all cover the
whole stream. The tradeoffs are that the spill file's disk usage grows with the
stream, and applying or clearing a filter does a full scan (which can take a
moment on a very large store).

## Recovering a corrupted display

`jlf tui` strips control characters from rendered content, so a stray carriage
return or escape inside a log field can't move the terminal cursor and corrupt
the frame. If something *outside* its control writes to the terminal — most often
the upstream program logging to **stderr** while its stdout is piped in — the
display can still be disturbed. Press `Ctrl-L` to force a full redraw, or send the
producer's stderr elsewhere:

```sh
myserver -f 2>/dev/null | jlf tui     # drop stderr
myserver -f 2>err.log   | jlf tui     # or keep it in a file
```
