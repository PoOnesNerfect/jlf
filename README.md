# jlf

[![Crates.io][crates-badge]][crates-url]
[![MIT licensed][mit-badge]][mit-url]

[crates-badge]: https://img.shields.io/crates/v/jlf.svg
[crates-url]: https://crates.io/crates/jlf
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/PoOnesNerfect/jlf/blob/main/LICENSE

**jlf** is a small, fast CLI for working with structured (JSON) logs: it reads
JSON/NDJSON from a pipe and pretty-prints, filters, redacts, summarizes, and
converts it — all streaming, single-pass, no backend.

Pipe your JSON logs through `jlf`:

```sh
cat ./examples/dummy_logs | jlf
```

What it does:

- **View** — turn JSON logs into colored, level-aware, human-readable output.
- **Filter** — keep matching records with `key=value` (`jlf level=error`).
- **Summarize** — `count`, `stats`, `top`, `uniq` over a stream, no backend.
- **Redact** — mask secrets/PII by field before sharing.
- **Convert** — export to CSV/TSV/Markdown.
- **Customize** — a small template DSL (fields, fallbacks, styles, conditionals,
  variables); extend with `jlf-<name>` plugins.

### Basic Example

**left**: `cat ./examples/dummy_logs`

**right**: `cat ./examples/dummy_logs | jlf`

<img width="1631" alt="Screenshot 2025-03-03 at 9 45 38 PM" src="https://github.com/user-attachments/assets/95b027e1-d005-48d7-a3c9-72b3b1be51b8" />

### At a glance

All examples below use this sample (`examples/sample.ndjson`):

```json
{"ts":"10:00:01","level":"info","msg":"login","user":"alice","latency_ms":42,"token":"abc123"}
{"ts":"10:00:02","level":"error","msg":"db timeout","user":"bob","latency_ms":510,"token":"xyz"}
{"ts":"10:00:03","level":"warn","msg":"retry","user":"alice","latency_ms":88}
{"ts":"10:00:04","level":"info","msg":"login","user":"carol","latency_ms":33,"token":"q9"}
{"ts":"10:00:05","level":"error","msg":"db timeout","user":"alice","latency_ms":620}
```

```sh
# view, compact: standard line + the rest as JSON
$ jlf -c < examples/sample.ndjson
info login {"ts":"10:00:01","user":"alice","latency_ms":42,"token":"abc123"}
error db timeout {"ts":"10:00:02","user":"bob","latency_ms":510,"token":"xyz"}
...

# filter to errors, custom template
$ jlf level=error '$ts $msg ($user)'
10:00:02 db timeout (bob)
10:00:05 db timeout (alice)

# count by level
$ jlf count level
     count  value
         2  info
         2  error
         1  warn
         5  total

# latency percentiles by level
$ jlf stats latency_ms by level
group                       count        min       mean        p50        p99
info                            2      33.00      37.50      42.00      42.00
error                           2     510.00     565.00     620.00     620.00

# export to CSV
$ jlf @csv ts,level,user
ts,level,user
10:00:01,info,alice
10:00:02,error,bob
...

# redact a secret field
$ jlf -c -r token
info login {"ts":"10:00:01","user":"alice","latency_ms":42,"token":"***"}
...

# top users
$ jlf top user
     count   share  value
         3   60.0%  alice
         1   20.0%  carol
         1   20.0%  bob
top 3 of 3 distinct (5 values)
```

## Installation

### Cargo

**cargo** is a rust's package manager.

To install **cargo**, visit [Install Rust - Rust Programming Language](https://www.rust-lang.org/tools/install)

```sh
cargo install jlf
```

### Manual Installation

You can also clone the repo and install it manually.

```sh
git clone https://github.com/PoOnesNerfect/jlf.git
cd jlf
cargo install --path crates/jlf --locked
```

## Table of Contents

<!--toc:start-->

- [Basic Example](#basic-example)
- [Installation](#installation)
  - [Cargo](#cargo)
  - [Manual Installation](#manual-installation)
- [Table of Contents](#table-of-contents)
- [CLI Options](#cli-options)
- [Input & Fields](#input--fields)
- [Filtering](#filtering)
- [Summaries](#summaries)
- [Extensions](#extensions)
- [Interactive viewer (jlf tui)](#interactive-viewer-jlf-tui)
- [Command builder (jlf it)](#command-builder-jlf-it)
- [Export](#export)
- [Redaction](#redaction)
- [Recipes](#recipes)
- [Usage](#usage)
  - [Compact Format](#compact-format)
  - [Color](#color)
  - [Strict](#strict)
- [Custom Formatting](#custom-formatting)
  - [Accessing Fields](#accessing-fields)
  - [Optional fields](#optional-fields)
  - [Styling and escape modifiers](#styling-and-escape-modifiers)
    - [Available Styles](#available-styles)
  - [Directives](#directives)
  - [Repetition](#repetition)
  - [Includes](#includes)
- [Config File](#config-file)
- [Neat Trick](#neat-trick)
- [Implementation](#implementation)
  - [JSON Parsing](#json-parsing)
    - [Some characteristics of common json logs:](#some-characteristics-of-common-json-logs)
    - [Optimizations](#optimizations)
    - [Benchmarks](#benchmarks)

<!--toc:end-->

## CLI Options

```
$ jlf -h

CLI for converting JSON logs to human-readable format

Usage: jlf [OPTIONS] [ARGS]... [COMMAND]

Commands:
  expand  Print a recipe with included recipes expanded. If none is specified, the default output is used
  list    List recipes
  count   Count lines, or a frequency breakdown of a field's values
  stats   Numeric summary of a field: count/min/max/mean/p50/p90/p99
  top     Most frequent values of a field (top N, default 10)
  uniq    Number of distinct values of a field
  help    Print this message or the help of the given subcommand(s)

Arguments:
  [ARGS]...  Format template, filters (key=value), and fields. [default: @output]

Options:
  -v, --variable <KEY=VALUE>  Override a recipe/variable as KEY=VALUE; can be passed multiple times
      --color <COLOR>         Color output: auto (default), always, or never [default: auto] [possible values: auto, always, never]
  -n, --no-color              Disable color output (shortcut for --color=never)
  -c, --compact               Display log in a compact format
  -s, --strict                On invalid JSON, report and exit non-zero instead of passing the line through
  -t, --take <TAKE>           Take only the first N emitted records
  -i, --input <FILE>          Input file(s); repeatable. Defaults to stdin
  -f, --fields <FIELDS>       Fields/columns to show (comma-separated), e.g. -f ts,level,msg
  -r, --redact <FIELDS>       Redact fields by name; comma-separated globs (e.g. password,token,*.email)
      --format <NAME>         Select an output format recipe by name (csv/tsv/md or custom). Usually written as @NAME
  -p, --preset <NAME>         Use a saved recipe from [recipe.NAME] (also: @NAME as a bare arg)
  -h, --help                  Print help
  -V, --version               Print version
```

## Input & Fields

By default **jlf** reads from stdin. Pass one or more files with `-i`/`--input`
(repeatable) to read them as a single stream — handy when you don't want a pipe:

```sh
jlf -i app.log                       # read a file instead of stdin
jlf -i app.log -i app.log.1 count    # concatenate files, then summarize
```

`-f`/`--fields` is a shortcut for projecting a few fields without writing a
template: `-f a,b,c` is equivalent to the template `'$a $b $c'`.

```sh
$ jlf -i examples/sample.ndjson -f ts,level,user
10:00:01 info alice
10:00:02 error bob
10:00:03 warn alice
10:00:04 info carol
10:00:05 error alice
```

`-t`/`--take N` stops after the first N emitted records (after filtering):

```sh
$ jlf -i examples/sample.ndjson -t 2 -f ts,level
10:00:01 info
10:00:02 error
```

## Filtering

Pass `key=value` arguments to keep only matching records. Operators: `=` `!=`
`>` `<` `>=` `<=`, `~` (contains), `!~` (not contains). Multiple filters AND
together; a comma-separated value is OR; nested fields use `.`.

```sh
jlf level=error                 # only errors
jlf level=error,warn status=500 # (error OR warn) AND status 500
jlf latency_ms>500              # numeric comparison (quote in a shell: 'latency_ms>500')
jlf message~timeout             # substring match
```

Worked example — keep only ERROR lines and show fields:

```sh
$ jlf level=error '$ts $msg ($user)' < examples/sample.ndjson
10:00:02 db timeout (bob)
10:00:05 db timeout (alice)

# numeric comparison — slow requests (quote filters with > or < for the shell)
$ jlf 'latency_ms>100' '$ts $user ${latency_ms}ms' < examples/sample.ndjson
10:00:02 bob 510ms
10:00:05 alice 620ms

# OR within a field, AND across filters
$ jlf -c level=error,warn user=alice < examples/sample.ndjson
warn retry {"ts":"10:00:03","user":"alice","latency_ms":88}
error db timeout {"ts":"10:00:05","user":"alice","latency_ms":620}

# substring (~), and negation (!=)
$ jlf -c 'msg~timeout' < examples/sample.ndjson   # only "db timeout" lines
$ jlf -c user!=alice    < examples/sample.ndjson  # exclude alice
```

## Summaries

Quick analytics over a stream, single pass, no backend:

```sh
jlf count                # number of (matching) lines
jlf count level          # breakdown by level, most frequent first
jlf top user_id 5        # 5 most frequent values + share
jlf uniq session_id      # distinct count
jlf stats latency_ms     # count/min/max/mean/p50/p90/p99
jlf stats latency_ms by endpoint   # grouped summary
jlf count type level=error         # filters apply to summaries too
```

Worked examples (using `examples/sample.ndjson`):

```sh
$ jlf count level
     count  value
         2  info
         2  error
         1  warn
         5  total

$ jlf top user
     count   share  value
         3   60.0%  alice
         1   20.0%  carol
         1   20.0%  bob
top 3 of 3 distinct (5 values)

$ jlf uniq user
3 distinct (of 5 values)

$ jlf stats latency_ms
count 5
min   33.00
max   620.00
mean  258.60
p50   88.00
p90   620.00
p99   620.00

$ jlf stats latency_ms by level
group                       count        min       mean        p50        p99
info                            2      33.00      37.50      42.00      42.00
error                           2     510.00     565.00     620.00     620.00
warn                            1      88.00      88.00      88.00      88.00

# filters apply to summaries too — top users among errors
$ jlf top user level=error
     count   share  value
         1   50.0%  bob
         1   50.0%  alice
top 2 of 2 distinct (2 values)
```

All summaries run in a single pass with bounded memory. `stats` keeps exact
percentiles for typical inputs and automatically switches to a t-digest sketch
past ~50k values per group, so percentiles stay constant-memory on huge streams
(`count`, `min`, `max`, and `mean` remain exact either way).

## Extensions

`jlf` dispatches unknown subcommands to `jlf-<name>` on your `PATH` (git-style),
so the core stays small and optional features install separately. `jlf foo` runs
`jlf-foo`; if it isn't installed, `jlf` prints an install hint.

The interactive viewer below (`jlf tui`) is the first such extension.

## Interactive viewer (`jlf tui`)

`jlf-tui` is a full-screen terminal app that brings viewing, filtering,
redaction, and summaries together with vi-style keys. It **live-tails** its
input: records show up as they stream in, so it works on a growing file or a
pipe.

```sh
cargo install jlf-tui        # or: cargo install --path crates/jlf-tui

tail -f app.log | jlf tui    # follow a live stream
jlf tui app.log              # open a file (keeps following appends)
jlf tui app.log level=error  # start with a filter applied
```

Layout: a scrolling record list on the left, a pretty-printed detail pane for
the selected record on the right, a status bar (follow state, position, active
filter), and a prompt line.

Keys:

| Key | Action |
| --- | ------ |
| `j` / `k`, `↓` / `↑` | move selection |
| `g` / `G` | jump to top / bottom |
| `Ctrl-d` / `Ctrl-u` | half-page down / up |
| `J` / `K` | scroll the detail pane |
| `f` | toggle follow (auto-scroll to newest) |
| `/` | filter — type `key=value` (same operators as the CLI), `Enter` applies |
| `:` | command — `count [field]`, `stats field`, `top field [n]`, `uniq field`, `redact a,b`, `csv\|tsv\|md cols [path]`, `q` |
| `Esc` | close a summary popup, or clear the filter |
| `q` | quit |

For example, press `/`, type `level=error`, `Enter` to keep only errors, then
`:top user` to see the top users among them, or `:csv ts,level,msg` to write the
current filtered view to `jlf-export.csv`.

## Command builder (`jlf it`)

`jlf it` is an interactive builder (like `npm create`) that assembles a command
step by step, shows a **live preview** of its output against a sample, and then
lets you run it, save it as a recipe, or both.

```sh
jlf it app.log          # build against a file
cat app.log | jlf it    # ...or a pipe (drained for the sample)
jlf it                  # ...or pick a sample interactively
```

It walks you through a mode (**View** / **Summarize** / **Export**), filters,
and mode-specific options; after each build it runs the real `jlf` on the sample
so you see exactly what you'll get. At the end you can **run it**, **save it as a
recipe**, or both. Saving appends a `[recipe.NAME]` block to your workspace
`.jlf.toml` or user config, so it's immediately usable:

```
✓ saved @errors → .jlf.toml
  run it any time with: jlf @errors
```

`jlf it` needs `jlf-it` on your `PATH` (`cargo install jlf-it`), like other
extensions.

## Export

Output formats are recipes with one `body` template. The built-in `csv`, `tsv`,
and `md` formats are recipes, so they run the same way as any saved recipe:

```sh
jlf @csv ts,level,message        # CSV with header; cells use :csv escaping
jlf @tsv ts,level,message        # TSV
jlf @md ts,level,message         # Markdown table
jlf @csv ts,level level=error    # filters apply
jlf @csv -f ts,level             # columns via -f also work
```

Columns come from a comma-list, `-f`/`--fields`, or leftover bare words. A table
format needs columns because its `$cols(...)` repetitions iterate the selected
column list. Bare `$(...)` repeats the current record's fields instead. A single
column can be a bare word: `jlf @csv ts`.

The built-in output formats are seeded from these definitions:

```toml
[recipe.csv]
body = "$cols( $key ),*\n$rows( $cols( ${value:csv} ),* )*"

[recipe.tsv]
body = "$cols( $key )\t*\n$rows( $cols( ${value:tsv} )\t* )*"

[recipe.md]
body = "| $cols( $key )\" | \"* |\n| $cols( --- )\" | \"* |\n$rows( | $cols( ${value:md} )\" | \"* | )*"
```

In these templates, `$cols( $key ),*` before `$rows(` writes the column-name
header row once, and `$rows( $cols( ${value:csv} ),* )*` writes one data row per
record.

Worked example — selected columns to CSV (RFC-4180 quoting):

```sh
$ jlf @csv ts,level,user < examples/sample.ndjson
ts,level,user
10:00:01,info,alice
10:00:02,error,bob
10:00:03,warn,alice
10:00:04,info,carol
10:00:05,error,alice

# Markdown table
$ jlf @md level,user < examples/sample.ndjson
| level | user |
| --- | --- |
| info | alice |
| error | bob |
| warn | alice |
| info | carol |
| error | alice |

# filters apply; pipe to a file/spreadsheet
$ jlf @csv ts,latency_ms level=error > errors.csv
```

Summaries take the same `@name` token: `jlf count level @md`,
`jlf stats latency_ms by level @csv`.

## Redaction

Mask field values by name before sharing, with `-r`/`--redact` (comma-separated
globs; `*.email` matches that key at any depth). Nested objects are handled:

```sh
jlf -r password,token,*.email    # password/token/email values become ***
```

Worked example:

```sh
$ jlf -c -r token < examples/sample.ndjson
info login {"ts":"10:00:01","user":"alice","latency_ms":42,"token":"***"}
error db timeout {"ts":"10:00:02","user":"bob","latency_ms":510,"token":"***"}
...
```

## Recipes

A recipe is a named, reusable definition. It replaces separate variables,
presets, and custom output formats with one config shape. Refer to recipes as
`@name`:

- `jlf @name` runs a recipe (filter → summarize/render → `body` template).
- Use an include directive such as `${ @name }` inside another template to inline
  that recipe's `body`.

```toml
# .jlf.toml — a recipe can be as small as a fragment or as full as a command

[recipe.errors]                 # a saved command: filter + layout
filter = "lvl|level|severity=error,fatal"
body   = "$ts $level $message"

[recipe.latency]                # a named field (one place for its aliases)
field  = "latency_ms|duration|elapsed"

[recipe.slow]                   # a summary
filter = "status>=500"
stats  = "latency_ms"
by     = "endpoint"
format = "md"

[recipe.htable]                 # an output format with dynamic columns
body = "<table>\n$rows( <tr>$cols( <td>${value:html}</td> )*</tr> )*</table>"
```

```sh
jlf @errors                     # filter to errors/fatals, render with body
jlf @slow                       # grouped latency stats as a Markdown table
jlf @htable ts,level,msg        # HTML table using selected columns
jlf '${ @level } $message'      # inline a recipe's layout

# named fields work in render, filter, and summary positions:
jlf '${ @latency }ms $message'
jlf 'latency_ms|duration|elapsed>500'
jlf stats latency_ms
```

Recipes are a starting point, not a fixed command — explicit args layer on top:

```sh
jlf @errors status=500          # add a filter (AND with the recipe's)
jlf @errors level=warn          # override the same-field filter
jlf @errors '$ts $msg'          # override the recipe's body
```

### Recipe keys

| key | purpose |
| --- | ------- |
| `body` | template; per-record by default, or whole-stream when it uses `$rows(...)`; `${ @other }` inlines another recipe body |
| `fields` | comma list; shorthand for a `$a $b $c` body and the columns used by `$cols(...)` |
| `field` / `style` | name a value (`a\|b.c`) and how to render it (`level`, `dimmed`, `json`) |
| `filter` | records to keep (same operators as CLI filters) |
| `redact`, `compact` | mask fields; force compact |
| `escape` | default escape modifier for interpolated values; also marks the recipe as a format |
| `count` / `stats` / `top` / `uniq`, `by`, `n` | run a summary |
| `format` | render through another output format recipe (`csv`/`tsv`/`md` or custom) |
| `base` | inherit another recipe (`@other`), then override its keys |

A `[recipes]` table is shorthand for body-only recipes (`name = "$a $b"`).

### Output formats are recipes too

A `[recipe.NAME]` is treated as an output format when it has `escape` or its
single `body` template uses `$rows(...)` or `$cols(...)`. `$rows(...)` is the
stream-framing mechanism: anything before it is written once before the first
record, the body inside it is written once per record, and anything after it is
written once after the stream.

`$cols( body )"sep"OP` repeats over the selected columns. Inside the repetition,
`$key` is the column name and `$value` is the cell value. `OP` is required and is
`*`, `+`, or `?`; the join text sits between `)` and the operator. Use quotes
when the join text contains spaces.

```toml
[recipe.psv]
body = "$cols( $key )\"|\"*\n$rows( $cols( ${value:csv} )\"|\"* )*"
```

```sh
jlf @psv -f ts,level,msg      # custom dialect, dynamic columns
jlf @csv ts,level,msg         # built-in, same mechanism
```

Escaping is a value modifier: `:csv`, `:tsv`, `:md`, `:html`, or `:none`. A
recipe may also set `escape = "html"` to apply a default to all interpolations in
that recipe.

### Conditional overrides

A `[recipe.NAME.<cond>]` sub-table overrides keys when a condition holds (a
config flag — `compact`, `no_color`, or `strict`), so simple configuration
branches can stay out of template strings:

```toml
[recipe.output]
body = "${ ?@timestamp } ${ ?@level } ${ ?@message }\n${ ?@data }"

[recipe.output.compact]         # only the join text changes under --compact
body = "${ ?@timestamp } ${ ?@level } ${ ?@message } ${ ?@data }"
```

The repo's `.jlf.toml` shows the default recipes.

## Usage

### Compact Format

By default, **jlf** prints the standard log in the first line, then rest of json data in a pretty format in the following lines.

If you want to print everything in a single line, you can pass the option `-c`/`--compact`.

```sh
cat ./examples/dummy_logs | jlf -c
```

<img width="700" alt="Screenshot 2025-03-03 at 11 01 27 PM" src="https://github.com/user-attachments/assets/b6f9ebe3-1f51-4a5e-9127-5b55a5b0e0a6" />

### Color

By default (`--color=auto`), **jlf** prints in pretty colors to a terminal and
strips colors when the output is piped to a file or another program, so files
aren't corrupted with ANSI characters.

Override with `--color`:

```sh
# auto (default): color on terminal, plain when piped
cat ./examples/dummy_logs | jlf

# force color even when piping into a color-capable pager
cat ./examples/dummy_logs | jlf --color=always | less -R

# never color (same as -n / --no-color)
cat ./examples/dummy_logs | jlf --color=never
cat ./examples/dummy_logs | jlf -n
```

<img width="700" alt="Screenshot 2025-03-03 at 11 07 47 PM" src="https://github.com/user-attachments/assets/7bebd267-6bca-4fe2-9102-e4dbc8416a44" />

### Strict

When **jlf** encounters log lines that are not valid JSON, it will simply pass the line through without any transformation.

However, if you would rather like to exit with an error when encountered an invalid JSON or a non-JSON line, pass the option `-s`/`--strict`.

It will even print out a snippet of where the JSON is invalid.

```sh
# pass `-s` to exit when non-JSON is found
cat ./examples/dummy_logs | jlf -s
```

<img width="700" alt="Screenshot 2025-03-03 at 11 20 49 PM" src="https://github.com/user-attachments/assets/640cea33-3197-4e78-b452-37883a2243c6" />

## Custom Formatting

Templates use a `$`-based DSL. Plain text is literal. `$` introduces
interpolation, and `{`/`}` are ordinary characters unless they are part of a
`${ … }` interpolation. Use `$$` for a literal dollar sign.

```sh
# Provide custom format. If `data` exists, print it as JSON; otherwise print a fallback.
cat ./examples/dummy_logs | jlf '${if data}${data:json}${else}`data` field not found${/}'
```

The formatting rules cover fields, modifiers, conditionals, includes, and
repetition.

### Accessing Fields

Use `$name` for a simple field. Use `${ … }` for nested paths, fallback chains,
modifiers, whole-record output, and rest output.

```sh
# the commands below pipe in this example line:
line='{"message": "User logged in successfully", "body": "My Body", "data": {"user_id": 3175, "session_id": "Nsb3P5mZ7971NFIt", "ip_address": "149.215.200.169", "friends":["Jack","Jill"]}}'

# bare field
echo "$line" | jlf 'Msg: $message!' # -> Msg: User logged in successfully!

# fallback chain: first present field wins
echo "$line" | jlf 'Msg: ${msg|body|message}!' # -> Msg: My Body!

# nested field
echo "$line" | jlf 'User ${data.user_id} logged in!' # -> User 3175 logged in!

# array index
echo "$line" | jlf 'My friend is ${data.friends[1]}.' # -> My friend is Jill.

# objects and arrays render as JSON by default
echo "$line" | jlf 'user data: $data'

# whole record
echo "$line" | jlf 'user(${data.user_id}): $message\n${.}'

# fields not already consumed by the template
echo "$line" | jlf 'user(${data.user_id}): $message\n${..}'
```

### Optional fields

Prefix a braced field expression with `?` to make it optional. When an optional
interpolation renders empty because the field is absent, `null`, or an empty
string, **jlf** collapses one adjacent space so missing fields leave no extra gap.

```sh
# `req_id` is optional: present on some lines, missing on others
echo '{"level":"info","req_id":"abc","msg":"ok"}' | jlf '$level ${ ?req_id } $msg'
# -> info abc ok

echo '{"level":"info","msg":"ok"}'                | jlf '$level ${ ?req_id } $msg'
# -> info ok          (no double space where req_id would be)
```

Optional fields support fallbacks and modifiers like any other field, such as
`${ ?trace_id|span_id }` and `${ ?level:level }`. Optional includes use the same
prefix form, for example `${ ?@data }`.

### Styling and escape modifiers

Modifiers follow `:` in a braced interpolation. Styling modifiers include
`dimmed`, `bold`, colors, and `level`. Escape modifiers make per-cell output safe
for a target format: `:csv`, `:tsv`, `:md`, `:html`, and `:none`.

```sh
cat ./examples/dummy_logs | jlf '${timestamp:bright blue,bg=red,bold} ${level|lvl:level} ${message|msg|body:fg=bright white}'
```

If you have multiple styles, separate them with `,`, like `fg=red,bg=blue`. If
you omit the style type before `=`, it defaults to `fg`.

#### Available Styles

- `dimmed`: make the text dimmed
- `bold`: make the text bold
- `fg=<color>`: set the text color
- `<color>`: same as `fg=<color>`
- `bg=<color>`: set the background color
- `indent=<n>`: indent the value by `n` spaces
- `key=<color>`: sets the color of the key in JSON object
- `value=<color>`: sets the color of the non-string types in JSON object
- `str=<color>`: sets the color of the string data type in JSON object
- `syntax=<color>`: sets the color of the syntax characters in JSON object
- `json`: print the JSON value as JSON
- `compact`: print JSON on a single line
- `level`: color the level based on the level (debug = green, info = cyan, etc.)

You can view all available colors in [colors.md](https://github.com/PoOnesNerfect/jlf/blob/main/colors.md).

### Directives

Directives use `${ … }` forms. Conditions end with `${/}`.

```sh
# if: truthy check; empty strings, empty arrays/objects, null, missing, and 0 are false
echo "$line" | jlf '${if msg}msg: $msg${else if message}message: $message${/}'

# key: existence check; falsey values still count when the key exists
echo "$line" | jlf '${key body}body = $body${else}no body${/}'

# config: branch on compact/no_color/strict
echo "$line" | jlf '$message${config compact} ${else}\n${/}${..}'
```

### Repetition

Repetition has four sources. The identifier between `$` and `(` selects what is
iterated; omitting the identifier means the current record itself.

`$( body )"sep"OP` repeats over the current record's top-level fields. Inside,
`$key` is the field name and `$value` is the field value.

`$path( body )"sep"OP` repeats over an object or array in each record. `$key` is
the object key or array index, and `$value` is the entry value. Sub-paths of the
value work, for example `$spans( ${value.name} )", "*`.

`$cols( body )"sep"OP` repeats over the CLI-selected columns from a comma-list,
`-f`/`--fields`, or leftover bare words. Inside, `$key` is the column name and
`$value` is the column value. `$cols(...)` has no entries unless columns are
selected.

`$rows( body )"sep"OP` repeats over the record stream. Text before `$rows(` is
written once before the first record, text after it is written once after the
stream, and the body is rendered once per record. This is how a format writes a
once-only header row or final wrapper.

For all repetition forms, `OP` is required: `*` for zero or more, `+` for one or
more, `?` for zero or one. The join text sits between `)` and the operator;
quote it when spaces should be visible.

```toml
[recipe.csv]
body = "$cols( $key ),*\n$rows( $cols( ${value:csv} ),* )*"
```

```toml
[recipe.output]
body = "${?timestamp:dimmed} ${?level:level} ${?target:dimmed}  $fields( $key=${value:dimmed} )\" \"*"
```

For a record with `fields = {"message":"ok","dir":"/d","n":5}`, this flattens
the nested entries, such as `message=ok dir=/d n=5`. Array fields use the same
form, for example `$errors( [$key]=$value )", "*`.

### Includes

Use an include directive such as `${ @name }` to inline another recipe's `body`
inside a template. The optional form `${ ?@name }` renders empty when the included
recipe renders empty and collapses one adjacent space.

```toml
[recipe.output]
body = "${ ?@timestamp } ${ ?@level } ${ ?@message }\n${ ?@data }"

[recipe.timestamp]
field = "timestamp"
style = "dimmed"

[recipe.level]
field = "level|lvl|severity"
style = "level"

[recipe.message]
field = "message|msg|body|fields.message"

[recipe.data]
field = ".."
style = "json"
```

You can inspect recipes with `jlf list` and expand one with `jlf expand NAME`.

## Config File

Default config values are written in [PoOnesNerfect/jlf/.jlf.toml](https://github.com/PoOnesNerfect/jlf/blob/main/.jlf.toml).

Feel free to copy this into your config directory, like `$XDG_CONFIG_HOME/jlf/config.toml`, or your workspace directory as `.jlf.toml` or `jlf.toml`.

_**jlf.toml**_

```toml
# Default config values
[config]
format   = "@output"
compact  = false
no_color = false
strict   = false

# Default recipes
[recipe.output]
body = "${ ?@timestamp } ${ ?@level } ${ ?@message }\n${ ?@data }"

[recipe.timestamp]
field = "timestamp"
style = "dimmed"

[recipe.level]
field = "level|lvl|severity"
style = "level"

[recipe.message]
field = "message|msg|body|fields.message"

[recipe.data]
field = ".."
style = "json"
```

## Neat Trick

Given that:

- If the input line is not a JSON, **jlf** will print the line as is.
- **jlf** removes all ANSI escape codes when piping to a file.

This means, you can just use `jlf` for non-JSON logs to pipe logs to a file with all the ansi escape codes removed.
When you just pipe it to a terminal, it will still style the logs as before.

Neat, right?

## Implementation

### JSON Parsing

The program cannot assume what the data structure of the incoming JSON logs will be.
There is no guarantee that the application that is piping the logs uses the best practices for logging,
or keep the consistent structure.

Thus, it must be able to parse any JSON log dynamically; that leaves us with having to use `serde_json::Value`.

But, can we do better? The answer is yes.

Although we cannot assume the data structure of the logs, we can still optimize for the common characteristics of JSON logs.
So, I decided to make a custom JSON parser that is optimized for JSON logs.

#### Some characteristics of common json logs:

Below are some characteristics of common json logs that I thought I could optimize for:

1. each log line is usually not super huge:
2. log lines usually have similar structures:
3. we don't need to transform data; we just reformat them.

#### Optimizations

Below are the optimizations I implemented for the corresponding items above:

1. JSON objects are parsed into vec of key-value pairs instead of map.
   - this way, we don't have to allocate memory for each key and value.
2. Since each line of JSON log has a similar structure, we can reuse the existing vecs that are already allocated.
   - we don't have to allocate memory for each line.
3. Don't validate primitive values, since we don't need to transform the data.
4. Instead of allocating new `String`s for each key and value, we use `&str` slices of the log string.

#### Benchmarks

So, how did it perform? That's the only thing that matters.

```
custom parse time: [987.52 ns 993.59 ns 1.0006 µs]
Found 12 outliers among 100 measurements (12.00%)
9 (9.00%) high mild
3 (3.00%) high severe

serde value parse time: [2.8045 µs 2.8357 µs 2.8729 µs]
Found 8 outliers among 100 measurements (8.00%)
4 (4.00%) high mild
4 (4.00%) high severe

serde structured parse time: [712.16 ns 714.93 ns 717.54 ns]
```

First section is the custom parse, second is the parsing into `serde_json::Value` parse and third is deserializing into a structured rust object.

The time is how long it took to deserialize a single line of json log.

The custom parser is about 3x faster than `serde_json::Value` parsing in this benchmark. It is still slower than structured parsing, but it keeps dynamic JSON support.
