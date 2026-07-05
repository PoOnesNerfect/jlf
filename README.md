# jlf

`jlf` turns JSON logs into something you can read. Pipe a stream of JSON lines in
and it prints a compact, colored, human-readable view — and the same command can
filter, summarize, redact, and export those logs.

```sh
# raw JSON in…
$ tail -f app.log
{"timestamp":"2026-07-05T18:17:10.430Z","level":"INFO","message":"started","port":8080}
{"timestamp":"2026-07-05T18:17:11.201Z","level":"error","msg":"db timeout","retries":3}

# …readable out
$ tail -f app.log | jlf
2026-07-05T18:17:10.430Z INFO started
{
  "port": 8080
}
2026-07-05T18:17:11.201Z error db timeout
{
  "retries": 3
}
```

It works on a growing file or a live pipe, colors on a terminal and stays plain
when piped, and prints non-JSON lines through untouched — so you can leave it in
front of any log stream.

## Highlights

- **View** — a readable line per record (timestamp, level, message) with the
  rest as JSON; colored on a terminal, plain when piped.
- **Filter** — keep records with `key=value` (`jlf level=error`), with numeric,
  substring, negation, AND/OR, and fallback-field forms.
- **Summarize** — `count`, `stats` (percentiles), `top`, and `uniq` over any
  field, optionally grouped.
- **Export** — CSV / TSV / Markdown tables, and field redaction.
- **Customize** — a small `$`-based template language for custom layouts and
  output formats, saved as named **recipes** in a config file.
- **Interactively** — a full-screen viewer (`jlf-tui`) and a command builder
  (`jlf-it`).
- **Fast** — a JSON parser tuned for log lines (~3× faster than
  `serde_json::Value` on typical logs).

## Contents

- [Install](#install)
- [Quick start](#quick-start)
- [Reading logs](#reading-logs)
- [Filtering](#filtering)
- [Selecting fields](#selecting-fields)
- [Summaries](#summaries)
- [Exporting](#exporting)
- [Interactive viewer (`jlf-tui`)](#interactive-viewer-jlf-tui)
- [Command builder (`jlf-it`)](#command-builder-jlf-it)
- [Custom formatting](#custom-formatting)
- [Recipes and configuration](#recipes-and-configuration)
- [How it works](#how-it-works)
- [More docs](#more-docs)

## Install

```sh
cargo install jlf                # the core CLI
cargo install jlf-tui            # optional: interactive viewer
cargo install jlf-it             # optional: command builder
```

Or from a clone:

```sh
cargo install --path crates/jlf
cargo install --path crates/jlf-tui
cargo install --path crates/jlf-it
```

## Quick start

```sh
# pretty-print a stream (the default)
tail -f app.log | jlf

# keep only errors
jlf level=error -i app.log

# a frequency breakdown by level
jlf count level -i app.log

# latency percentiles
jlf stats latency_ms -i app.log

# export selected columns as CSV
jlf @csv ts,level,message -i app.log
```

Input comes from stdin or one or more `-i/--input` files. Everything else is a
positional argument, classified by shape: a token with `$` is a **template**, a
token with an operator (`level=error`) is a **filter**, a comma-list
(`ts,level`) selects **fields**, and `@name` runs a **recipe**. The first
subcommand (`count`, `stats`, `top`, `uniq`) switches to summary mode.

## Reading logs

With no template, `jlf` renders the default view: `timestamp`, `level`, and
`message` on the first line (level colored by severity), then every remaining
field as JSON below.

```sh
printf '%s\n' \
  '{"timestamp":"t1","level":"WARN","message":"slow","code":5}' \
  '{"note":"raw, no standard fields"}' | jlf
```
```text
t1 WARN slow
{
  "code": 5
}
{
  "note": "raw, no standard fields"
}
```

A record without timestamp/level/message collapses those slots and just shows
the JSON. Common options:

| flag | effect |
| ---- | ------ |
| `-c`, `--compact` | keep the trailing JSON inline instead of pretty-printed |
| `--color <auto\|always\|never>` | color mode; `-n`/`--no-color` is `never` |
| `-t`, `--take <N>` | stop after the first N records |
| `-i`, `--input <FILE>` | read from files (repeatable); default is stdin |
| `-s`, `--strict` | exit on a non-JSON line instead of printing it as-is |

```sh
printf '{"timestamp":"t1","level":"WARN","message":"slow","code":5}\n' | jlf -c
# -> t1 WARN slow {"code":5}
```

Because non-JSON lines pass through unchanged, and color is dropped when the
output is piped, `some-app | jlf > app.log` is a handy way to strip ANSI codes
from a mixed stream while still coloring it live on screen.

## Filtering

A filter is `key OP value`. Filters keep matching records; multiple filters are
ANDed.

```sh
jlf level=error                 # exact match
jlf 'latency_ms>500'            # numeric (quote > and < for the shell)
jlf msg~timeout                 # substring contains
jlf level!=info                 # negation
jlf level=error user=alice      # AND across tokens
jlf level=error,warn            # OR within a field (comma)
jlf 'lvl|level|severity=error'  # fallback fields — first present wins
```

Operators: `=` `!=` (string), `>` `>=` `<` `<=` (numeric), `~` `!~` (substring).
Nested keys use dots (`data.user.id=7`).

```sh
printf '%s\n' \
  '{"level":"info","user":"alice","latency_ms":42}' \
  '{"level":"error","user":"bob","latency_ms":510}' \
  '{"level":"error","user":"alice","latency_ms":620}' | jlf level=error -c
```
```text
error {"user":"bob","latency_ms":510}
error {"user":"alice","latency_ms":620}
```

## Selecting fields

`-f`/`--fields` (or a bare comma-list) picks which fields to show, in order:

```sh
printf '{"ts":"t1","level":"info","user":"alice","latency_ms":42}\n' | jlf -f ts,level,user
# -> t1 info alice
```

The selected fields are also the columns for table formats (`@csv`, `@md`) and
for `$cols(...)` in a template.

## Summaries

A summary subcommand comes first; filters still apply.

```sh
jlf count                       # count matching lines
jlf count level                 # frequency breakdown by a field
jlf stats latency_ms            # count/min/max/mean/p50/p90/p99
jlf stats latency_ms by level   # grouped stats
jlf top user                    # most frequent values (top N, default 10)
jlf uniq user                   # number of distinct values
jlf count level level=error     # filters apply to summaries too
```

```sh
printf '%s\n' \
  '{"level":"info","latency_ms":42}' \
  '{"level":"error","latency_ms":510}' \
  '{"level":"error","latency_ms":620}' | jlf stats latency_ms
```
```text
count 3
min   42.00
max   620.00
mean  390.67
p50   510.00
p90   620.00
p99   620.00
```

Percentiles are exact for normal inputs and switch to a t-digest approximation
past ~50k values per group (count/min/max/mean stay exact). A summary field
accepts a `|` fallback chain too (`stats latency_ms|duration`).

Render a summary as a table by adding a format:

```sh
jlf count level @csv
jlf stats latency_ms by endpoint @md
```

## Exporting

The built-in `csv`, `tsv`, and `md` formats turn selected columns into a table.
Run one by name (`@csv`) with a column list:

```sh
printf '%s\n' '{"a":"1","b":"x,y"}' '{"a":"2","b":"z"}' | jlf @csv a,b
```
```text
a,b
1,"x,y"
2,z
```

`@md` frames a GitHub-style table; `@tsv` uses tabs. Values are escaped for the
target format (the `x,y` above is quoted). Redirect to a file as usual:

```sh
jlf @csv ts,level,message -i app.log > out.csv
```

**Redaction** masks fields by name (comma-separated globs), for both the view
and exports:

```sh
printf '{"user":"bob","token":"secret"}\n' | jlf -c --redact token
# -> {"user":"bob","token":"***"}
```

Globs match nested paths too, e.g. `--redact password,*.email`.

## Interactive viewer (`jlf-tui`)

`jlf-tui` is a full-screen terminal app that brings viewing, filtering,
redaction, and summaries together with vi-style keys. It **live-tails** its
input, so it works on a growing file or a pipe.

```sh
tail -f app.log | jlf-tui     # follow a live stream
jlf-tui app.log               # open a file (keeps following appends)
jlf-tui app.log level=error   # start with a filter applied
```

Layout: a scrolling record list, a pretty-printed detail pane for the selected
record, a status bar (follow state, position, filter), and a prompt line.

| key | action |
| --- | ------ |
| `j`/`k`, `↓`/`↑` | move selection |
| `g`/`G` | jump to top / bottom |
| `Ctrl-d`/`Ctrl-u` | half-page down / up |
| `J`/`K` | scroll the detail pane |
| `f` | toggle follow (auto-scroll to newest) |
| `/` | filter — type `key=value`, `Enter` applies |
| `:` | command — `count [field]`, `stats field`, `top field [n]`, `uniq field`, `redact a,b`, `csv\|tsv\|md cols`, `q` |
| `Esc` | close a summary popup, or clear the filter |
| `q` | quit |

For example: `/` `level=error` `Enter` to keep errors, then `:top user`, or
`:csv ts,level,msg` to write the current view to `jlf-export.csv`.

## Command builder (`jlf-it`)

`jlf-it` is an interactive builder (like `npm create`) that assembles a command
step by step, shows a **live preview** against a sample, then lets you run it,
save it as a recipe, or both.

```sh
jlf-it app.log          # build against a file
cat app.log | jlf-it    # ...or a pipe (drained for the sample)
jlf-it                  # ...or pick a sample interactively
```

It walks through a mode (View / Summarize / Export), filters, and options,
re-running the real `jlf` on the sample after each step. Saving appends a
`[recipe.NAME]` block to your `.jlf.toml` (or user config), so it's immediately
usable with `jlf @NAME`.

## Custom formatting

The default view is just a template. Provide your own to lay records out however
you like — `jlf` uses a small `$`-based template language.

```sh
printf '{"level":"INFO","user":"alice","latency_ms":42}\n' \
  | jlf '$level ${user} took ${latency_ms}ms'
# -> INFO alice took 42ms
```

Plain text is literal; `$` introduces interpolation. Two families make up the
language: `${ … }` interpolation, and self-closing `$name( … )` blocks
(repetition, conditionals, and `$match`). The block forms borrow their shape
from Rust's `macro_rules!`. Here's a tour; see **[docs/DSL.md](docs/DSL.md)** for
the complete reference with an example of every construct.

**Fields.** `$name`, nested `${a.b}`, fallbacks `${a|b|c}` (first present wins),
the whole record `${.}`, the leftovers `${..}`, and optional `${?field}` (an
absent field collapses one adjacent space).

```sh
echo '{"a":1,"b":2}' | jlf -c 'a=$a rest=${..}'
# -> a=1 rest={"b":2}
```

**Modifiers** follow a `:` — styling (`fg=cyan`, `dimmed`, `bold`, or a bare
color name), `:json` for object/array values, and escaping (`:csv`, `:tsv`,
`:md`, `:html`).

```sh
echo '{"v":"<x>"}' | jlf '${v:html}'
# -> &lt;x&gt;
```

**Conditionals** are self-closing blocks that chain by adjacency:

```sh
printf '{"status":200}\n{"status":404}\n{"status":503}\n' \
  | jlf '$if(status >= 500 => down)$elif(status >= 400 => client)$else(ok)'
# -> ok
# -> client
# -> down
```

- `$if(COND => body)` — truthy, or a comparison with `== != > >= < <=`.
- `$key(FIELD => body)` — existence (a present-but-falsey `0`/`""` still counts).
- `$config(FLAG => body)` — branch on `compact`/`no_color`/`strict`.
- `$path?(body)` — the terse "render if truthy" guard.

**Match** dispatches on a value, binding the subject to `$value`:

```sh
printf '{"s":200}\n{"s":503}\n' \
  | jlf '$match(s $when(>=500 => 5xx) $when(400..500 => 4xx) $else(ok:$value))'
# -> ok:200
# -> 5xx
```

Patterns are comparisons, numeric ranges (`400..500`), literals, and `|`
alternation. A missing subject matches no arm (not even `$else`).

**Repetition** iterates fields, an object/array, columns, or the record stream —
`$( … )`, `$path( … )`, `$cols( … )`, `$rows( … )` — with a `*`/`+`/`?` operator:

```sh
echo '{"a":1,"b":2}' | jlf '$( $key=$value )" "*'
# -> a=1 b=2
```

**Includes** inline another recipe: `${@name}` (and `${?@name}` to optionalize).

## Recipes and configuration

A **recipe** is a named, reusable definition — a layout fragment, a template, a
saved command, or an output format. It's the one shape that replaces separate
variables, presets, and formats. Refer to a recipe as `@name`:

- `jlf @name` runs it (filter → summarize/render → `body`).
- `${@name}` inlines its `body` in another template.

Recipes live in a config file: a workspace `.jlf.toml` / `jlf.toml`, or your user
config at `$XDG_CONFIG_HOME/jlf/config.toml`. Workspace values override user
values.

```toml
[recipe.errors]                 # a saved command: filter + layout
filter = "lvl|level|severity=error,fatal"
body   = "$ts $level $message"

[recipe.slow]                   # a summary
filter = "status>=500"
stats  = "latency_ms"
by     = "endpoint"
format = "md"
```

```sh
jlf @errors                     # filter to errors/fatals, render with body
jlf @slow                       # grouped latency stats as a Markdown table
```

A recipe is a starting point, not a frozen command — explicit args layer on top:

```sh
jlf @errors status=500          # add a filter (ANDed with the recipe's)
jlf @errors level=warn          # override the same-field filter
jlf @errors '$ts $msg'          # override the body
```

### Named fields — one definition, three uses

A recipe with a `field` names a value accessor (a fallback chain). Once named, it
works the same in **templates, filters, and summaries**:

```toml
[recipe.latency]
field = "latency_ms|duration|elapsed"
```

```sh
jlf '${@latency}ms $message'    # template: render it
jlf @latency>500                # filter: keep slow requests
jlf stats @latency              # summary: percentiles
jlf count @latency              # summary: value breakdown
```

Each resolves the same `latency_ms|duration|elapsed` chain, picking the first
present field.

### Recipe keys

All keys are optional; a recipe uses only the ones its role needs.

| key | purpose |
| --- | ------- |
| `body` | template; per-record, or whole-stream when it uses `$rows(...)`; `${@other}` inlines another recipe |
| `field` / `style` | name a value accessor (`a\|b.c`) and how to render it (`fg=cyan`, `dimmed`, `json`) |
| `fields` | comma list; shorthand for a `$a $b $c` body and the `$cols(...)` columns |
| `filter` | records to keep (same operators as CLI filters) |
| `count` / `stats` / `top` / `uniq` | run a summary over a field; `by` groups, `n` sets top-N |
| `escape` | default escape for interpolated values (`html`/`csv`/`tsv`/`md`/`none`); also marks the recipe as a format |
| `redact` / `compact` | mask fields; force compact |
| `format` | render through another output format recipe |
| `base` | inherit another recipe (`@other`), then override its keys |

### Output formats are recipes

An output format is a recipe whose `body` frames the stream with `$rows(...)`
(text before it prints once as a header, text after once as a footer) or that
sets a global `escape`. The built-in `csv`/`tsv`/`md` are seeded this way, and
custom formats are ordinary recipes:

```toml
[recipe.report]
escape = "html"
body = "<ul>\n$rows( <li>${level}: ${msg}</li>\n)*</ul>\n"
```

```sh
printf '{"level":"INFO","msg":"a<b"}\n' | jlf @report
# -> <ul>
# -> <li>INFO: a&lt;b</li>
# -> </ul>
```

### Conditional overrides

A `[recipe.NAME.<flag>]` sub-table overrides individual keys when a flag holds
(`compact`, `no_color`, `strict`). The default `output` uses this so `--compact`
keeps the JSON inline:

```toml
[recipe.output]
body = "${@timestamp} ${@level} ${@message}\n${@data}"

[recipe.output.compact]
body = "${@timestamp} ${@level} ${@message} ${@data}"
```

### The default configuration

These are the built-in defaults, and a good starting point to copy and tweak.
Inspect resolved recipes with `jlf list`, and expand one with `jlf expand NAME`.

```toml
[config]
format   = "@output"
compact  = false
no_color = false
strict   = false

# `output` joins the field recipes below. Each field recipe is optional by
# default, so an absent field collapses its space.
[recipe.output]
body = "${@timestamp} ${@level} ${@message}\n${@data}"

[recipe.output.compact]
body = "${@timestamp} ${@level} ${@message} ${@data}"

[recipe.timestamp]
field = "timestamp"
style = "dimmed"

# `level` dispatches on lvl|level|severity and colors it by severity; an unknown
# level renders uncolored, and a missing one renders nothing.
[recipe.level]
body = '''$match(lvl|level|severity
  $when("ERROR"|"error" => ${value:fg=red})
  $when("WARN"|"warn"   => ${value:fg=yellow})
  $when("INFO"|"info"   => ${value:fg=cyan})
  $when("DEBUG"|"debug" => ${value:fg=green})
  $when("TRACE"|"trace" => ${value:fg=cyan,dimmed})
  $else(${value})
)'''

[recipe.message]
field = "message|msg|body|fields.message"

[recipe.data]
field = ".."
style = "json"
```

The canonical copy lives at
[.jlf.toml](https://github.com/PoOnesNerfect/jlf/blob/main/.jlf.toml).

## How it works

`jlf` can't assume the shape of incoming logs, so it parses each line
dynamically. The usual tool for that is `serde_json::Value`, but logs have
properties worth optimizing for:

1. each line is small,
2. lines share a similar structure,
3. we reformat rather than transform the data.

So `jlf` uses a custom parser that:

1. parses objects into a vec of key/value pairs (not a map),
2. reuses the vecs already allocated for the previous line,
3. skips validating primitive values (we don't transform them),
4. borrows `&str` slices of the input line instead of allocating new strings.

On typical log lines it parses in roughly a third of the time of
`serde_json::Value`:

```text
custom parse:       ~0.99 µs / line
serde_json::Value:  ~2.84 µs / line
```

## More docs

- **[docs/DSL.md](docs/DSL.md)** — the complete template language reference, with
  a runnable example for every construct.
- **[docs/VISION.md](docs/VISION.md)** — the design and rationale.
