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
$ jlf level=error '{ts} {msg} ({user})'
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
$ jlf --csv ts,level,user
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
- [Export](#export)
- [Redaction](#redaction)
- [Usage](#usage)
  - [Compact Format](#compact-format)
  - [Color](#color)
  - [Strict](#strict)
- [Custom Formatting](#custom-formatting)
  - [Accessing Fields](#accessing-fields)
  - [Styling Fields](#styling-fields)
    - [Available Styles](#available-styles)
  - [Conditionals](#conditionals)
    - [{#if cond1}{:else if cond2}{:else}{/if}](#if-cond1else-if-cond2elseif)
    - [{#key field1}{:else key field2}{:else}{/key}](#key-field1else-key-field2elsekey)
    - [{#config config1}{:else}{/config}](#config-config1elseconfig)
  - [Variables](#variables)
    - [Storing Variables](#storing-variables)
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
  expand  Print variable with its inner variables expanded. If no variable is specified, the default format string will be used
  list    List all variables
  count   Count lines, or a frequency breakdown of a field's values
  stats   Numeric summary of a field: count/min/max/mean/p50/p90/p99
  top     Most frequent values of a field (top N, default 10)
  uniq    Number of distinct values of a field
  help    Print this message or the help of the given subcommand(s)

Arguments:
  [ARGS]...  Format template, filters (key=value), and fields. [default: {&output}]

Options:
  -v, --variable <KEY=VALUE>  Pass variable as KEY=VALUE format; can be passed multiple times
      --color <COLOR>         Color output: auto (default), always, or never [default: auto] [possible values: auto, always, never]
  -n, --no-color              Disable color output (shortcut for --color=never)
  -c, --compact               Display log in a compact format
  -s, --strict                On invalid JSON, report and exit non-zero instead of passing the line through
  -t, --take <TAKE>           Take only the first N emitted records
  -i, --input <FILE>          Input file(s); repeatable. Defaults to stdin
  -f, --fields <FIELDS>       Fields/columns to show (comma-separated), e.g. -f ts,level,msg
  -r, --redact <FIELDS>       Redact fields by name; comma-separated globs (e.g. password,token,*.email)
      --csv                   Output as CSV
      --tsv                   Output as TSV
      --md                    Output as a Markdown table
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
template: `-f a,b,c` is equivalent to the template `'{a} {b} {c}'`.

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
$ jlf level=error '{ts} {msg} ({user})' < examples/sample.ndjson
10:00:02 db timeout (bob)
10:00:05 db timeout (alice)

# numeric comparison — slow requests (quote filters with > or < for the shell)
$ jlf 'latency_ms>100' '{ts} {user} {latency_ms}ms' < examples/sample.ndjson
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

## Extensions

`jlf` dispatches unknown subcommands to `jlf-<name>` on your `PATH` (git-style),
so the core stays small and optional features install separately. `jlf foo` runs
`jlf-foo`; if it isn't installed, `jlf` prints an install hint.

## Export

Render records to other formats from a comma-list of columns:

```sh
jlf --csv ts,level,message       # CSV with header (cells escaped)
jlf --tsv ts,level,message       # TSV
jlf --md ts,level,message        # Markdown table
jlf --csv ts,level level=error   # filters apply
```

Worked example — selected columns to CSV (RFC-4180 quoting):

```sh
$ jlf --csv ts,level,user < examples/sample.ndjson
ts,level,user
10:00:01,info,alice
10:00:02,error,bob
10:00:03,warn,alice
10:00:04,info,carol
10:00:05,error,alice

# Markdown table
$ jlf --md level,user < examples/sample.ndjson
| level | user |
| --- | --- |
| info | alice |
| error | bob |
| warn | alice |
| info | carol |
| error | alice |

# filters apply; pipe to a file/spreadsheet
$ jlf --csv ts,latency_ms level=error > errors.csv
```

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

You can optionally provide your custom format of the output line.

```sh
# Provide custom format. If `data` field exists, print `data` field as `json`; if not, print "`data` field not found".
cat ./examples/dummy_logs | jlf '{#if data}{data:json}{:else}`data` field not found{/if}'
```

<img width="700" alt="Screenshot 2025-03-03 at 11 32 02 PM" src="https://github.com/user-attachments/assets/a24cee4d-c1af-4dec-801c-88f118566278" />

Isn't it neat? The formatting syntax is very simple and readble, inspired by popular formatting syntax from the likes of rust and svelte.

We'll go over all the formatting rules now: fields, styles, conditionals, and variables.

Especially, `variables` is a new addition in `jlf v0.2.0` which unlocked the power of granular customization.

### Accessing Fields

To print the fields of JSON log, simple write the field name in braces `{field1}`.

```sh
# the commands below pipe in this example line:
line='{"message": "User logged in successfully", "body": "My Body", "data": {"user_id": 3175, "session_id": "Nsb3P5mZ7971NFIt", "ip_address": "149.215.200.169", "friends":["Jack","Jill"]}}'

# access the field by writing the field in braces
echo "$line" | jlf 'Msg: {message}!' # -> Msg: User logged in successfully!

# if field may not exist, provide fallback fields separated by '|'. It will print the first field that exits.
echo "$line" | jlf 'Msg: {msg|body|message}!' # -> Msg: My Body!

# access nested field using '.' as a separator.
echo "$line" | jlf 'User {data.user_id} logged in!' # -> User 3175 logged in!

# access array items using '[n]' to index at `n`.
echo "$line" | jlf 'My girl friend is {data.friends[1]}.' # -> My girl friend is Jill.

# if the field is an object or array, it will it as pretty json by default.
echo "$line" | jlf 'user data: {data}'
# ->
# user data: {
#  "user_id": 3175,
#  "session_id": "Nsb3P5mZ7971NFIt",
#  "ip_address": "149.215.200.169",
#  "friends": [
#     "Jack",
#     "Jill"
#   ]
# }

# print the entire json by writing `{.}`
echo "$line" | jlf 'user({data.user_id}): {message}\n{.}'
# ->
# user(3175): User logged in successfully
# {
#   "message": "User logged in successfully",
#   "body": "My Body",
#   "data": {
#     "user_id": 3175,
#     "session_id": "Nsb3P5mZ7971NFIt",
#     "ip_address": "149.215.200.169",
#     "friends": [
#       "Jack",
#       "Jill"
#     ]
#   }
# }

# print only the un-printed fields by writing `{..}`
echo "$line" | jlf 'user({data.user_id}): {message}\n{..}'
# ->
# user(3175): User logged in successfully
# {
#   "body": "My Body",
#   "data": {
#     "session_id": "Nsb3P5mZ7971NFIt",
#     "ip_address": "149.215.200.169",
#     "friends": [
#       "Jack",
#       "Jill"
#     ]
#   }
# }
```

### Styling Fields

You can provide styles to the values by providing styles after the `:`.

```sh
cat ./examples/dummy_logs | jlf '{timestamp:bright blue,bg=red,bold} {level|lvl:level} {message|msg|body:fg=bright white}'
```

If you have multiple styles, you can separate them with `,`, like `fg=red,bg=blue`.

You can optionally provide the style type before the `=`. If you don't provide it, it will default to `fg`.

<img width="700" alt="Screenshot 2025-03-04 at 12 18 28 AM" src="https://github.com/user-attachments/assets/acc21974-695b-4cf7-ba27-c873f944356d" />

`level` is a special style that is only applied to `level` field; it will print in different colors for different levels.

#### Available Styles

- `dimmed`: make the text dimmed
- `bold`: make the text bold
- `fg={color}`: set the text color
- `{color}`: same as `fg={color}`
- `bg={color}`: set the background color
- `indent={n}`: indent the value by `n` spaces
- `key={color}`: sets the color of the key in JSON object
- `value={color}`: sets the color of the non-string types in JSON object
- `str={color}`: sets the color of the string data type in JSON object
- `syntax={color}`: sets the color of the syntax characters in JSON object
- `json`: print the json value as json; this is the default and only available format, so you don't have to specify it
- `compact`: print in a single line
- `level`: color the level based on the level (debug = green, info = cyan, etc.)

In the above list, `{color}` is a placeholder for any color value.

You can view all available colors in [colors.md](https://github.com/PoOnesNerfect/jlf/blob/main/colors.md).

### Conditionals

For conditionals, main conditional starts with `#` like `{#if ..}`, else conditions start with `:` like `{:else ..}`, and ending symbols start with `/` like `{/if}`.

#### {#if cond1}{:else if cond2}{:else}{/if}

**if** condition accepts a single field or multiple fields separated by '|'.

**if** checks for the `truthy`ness of the given field values; one difference with Javascript truthiness is that empty object and array is evaluated to `false`.

```sh
# the commands below pipe in this example line:
line='{"message": "User logged in successfully", "body": "", "data": {"user_id": 3175, "count": 0, "friends":[]}}'

# if field doesn't exist, or is null, it's `false`.
echo "$line" | jlf '{#if msg}msg: {msg}{:else if message}message: {message}{/if}' # -> message: User logged in successfully

# empty string is also `false`.
echo "$line" | jlf '{#if body}body = {body}{:else}no body{/if}' # -> no body

# number 0 is also 'false'.
echo "$line" | jlf '{#if data.count}count = {data.count}{:else}count is zero{/if}' # -> count is zero

# empty object or arrays are also 'false'.
echo "$line" | jlf '{#if data.friends}friends: {data.friends}{:else}I have no friends{/if}' # -> I have no friends

# nesting is allowed
echo "$line" | jlf '{#if data.user_id}user ({data.user_id}) {#if message}has a message{:else}has no message{/if}{/if}.' # -> user (3175) has a message.

# if multiple fields are given, it will return `true` if at least one of them is `truthy`.
echo "$line" | jlf "{#if msg|body|data.count|message}I'm still here{/if}" # -> I'm still here
```

#### {#key field1}{:else key field2}{:else}{/key}

**key** condition accepts a single field or multiple fields separated by '|'.

**key** checks the existence of the given field; even when the field value is `falsey`, it will evaluate to `true` if the field exists, and is not null.

```sh
# the commands below pipe in this example line:
line='{"message": "User logged in successfully", "body": "", "data": {"user_id": 3175, "count": 0, "friends":[]}}'

# if field doesn't exist, or is null, it's `false`.
echo "$line" | jlf '{#key msg}msg: {msg}{:else key message}message: {message}{/key}' # -> message: User logged in successfully

# empty string is still `true`.
echo "$line" | jlf '{#key body}body = {body}{:else}no body{/key}' # -> body = 

# number 0 is also 'true'.
echo "$line" | jlf '{#key data.count}count = {data.count}{:else}count is zero{/key}' # -> count = 0

# empty object or arrays are also 'true'.
echo "$line" | jlf '{#key data.friends}friends: {data.friends}{:else}I have no friends{/key}' # -> friends: []

# nesting is allowed
echo "$line" | jlf '{#key data.user_id}user ({data.user_id}) {#key message}has a message{:else}has no message{/key}{/key}.' # -> user (3175) has a message.

# if multiple fields are given, it will return `true` if at least one of them exists.
echo "$line" | jlf "{#key msg|no_field|message}I'm still here{/key}" # -> I'm still here
```

#### {#config config1}{:else}{/config}

**config** condition accepts a config: `compact`, `no_color`, or `strict`.

**config** returns `true` if the given config is set.

```sh
# the commands below pipe in this example line:
line='{"message": "User logged in successfully", "body": "", "data": {"user_id": 3175, "count": 0, "friends":[]}}'

# If `compact` is set, print ` `; if `compact` is not set, print `\n`
echo "$line" | jlf -c '{message}{#config compact} {:else}\n{/config}{..}'
# -> User logged in successfully {"body":"","data":{"user_id":3175,"count":0,"friends":[]}}

echo "$line" | jlf '{message}{#config compact} {:else}\n{/config}{..}'
# ->
# User logged in successfully
# {
#   "body": "",
#   "data": {
#     "user_id": 3175,
#     "count": 0,
#     "friends": []
#   }
# }

# {:else config ..} is not supported.
echo "$line" | jlf '{message}{#config compact} {:else config strict}strict{:else}\n{/config}{..}' # -> INVALID
```

### Variables

**variable** are key=value pairs, where `key` is a string, and `value` is a format string.

You can reference a variable in the format string or in another variable as `{&variable}`.

Here is the list of all default variables:

```toml
output    = "{#key timestamp|level|lvl|severity|message|msg|body|fields.message}{&timestamp}{&level}{&message}{#config compact} {:else}\\n{/config}{/key}{&data}"
timestamp = "{#key timestamp}{timestamp:dimmed} {/key}"
level     = "{#key level|lvl|severity}{level|lvl|severity:level} {/key}"
message   = "{message|msg|body|fields.message}"
data      = "{..:json}"
```

Each variable is the whole rendered piece named for what it is — no `name`/`name_fmt`
twins. A trailing space inside a `{#key …}{/key}` guard is omitted when the field is
absent, so missing fields leave no stray gap. You can see the variables with
`jlf list`.

When expanded, variable `output` will look like this:

```sh
{#key timestamp|level|lvl|severity|message|msg|body|fields.message}{#key timestamp}{timestamp:dimmed} {/key}{#key level|lvl|severity}{level|lvl|severity:level} {/key}{message|msg|body|fields.message}{#config compact} {:else}\n{/config}{/key}{..:json}
```

You can view the expanded variables by calling `jlf expand VARIABLE`.

For example, `jlf expand level` will output `{#key level|lvl|severity}{level|lvl|severity:level} {/key}`.

If you don't provide at variable, `jlf expand`, it will print the fully expanded format string.

```sh
# the commands below pipe in this example line:
line='{"timestamp": "2024-02-09T07:22:41.439284", "level": "DEBUG", "message": "User logged in successfully", "data": {"user_id": 3175}}'

echo "$line" | jlf
# ->
# 2024-02-09T07:22:41.439284 DEBUG User logged in successfully
# {
#   "data": {
#     "user_id": 3175
#   }
# }

# override variable `message`
echo "$line" | jlf -v message="Message: {message}"
# ->
# 2024-02-09T07:22:41.439284 DEBUG Message: User logged in successfully
# {
#   "data": {
#     "user_id": 3175
#   }
# }

# don't print timestamp by resetting variable `timestamp`
echo "$line" | jlf -v timestamp=
# ->
# DEBUG User logged in successfully
# {
#   "timestamp": "2024-02-09T07:22:41.439284",
#   "data": {
#     "user_id": 3175
#   }
# }

# pass multiple variables
echo "$line" | jlf -v timestamp= -v message="Message: {message}"
# ->
# DEBUG Message: User logged in successfully
# {
#   "timestamp": "2024-02-09T07:22:41.439284",
#   "data": {
#     "user_id": 3175
#   }
# }

# print the entire json instead of only unused fields
echo "$line" | jlf -v data="{.:json}"
# ->
# 2024-02-09T07:22:41.439284 DEBUG User logged in successfully
# {
#   "timestamp": "2024-02-09T07:22:41.439284",
#   "level": "DEBUG",
#   "message": "User logged in successfully",
#   "data": {
#     "user_id": 3175
#   }
# }

# replace the entire format (default is `{&output}`)
echo "$line" | jlf -v output="{message}: {&data}"
# ->
# User logged in successfully: {
#   "timestamp": "2024-02-09T07:22:41.439284",
#   "level": "DEBUG",
#   "data": {
#     "user_id": 3175
#   }
# }
```

As you can see, it's extremely easy to update the format either partially or wholly by replacing the default variables.

#### Storing Variables

This is all and good, but it may still become annoying to specify variables as commands options everytime.

Instead we can set the variables in the config file.

**jlf** looks for the config file `$XDG_CONFIG_HOME/jlf/config.toml` and `jlf.toml`/`.jlf.toml` in the current workspace.

Priority of config and variables is `Command options` > `jlf.toml`|`.jlf.toml` > `$XDG_CONFIG_HOME/jlf/config.toml`.

Default config values are written in [PoOnesNerfect/jlf/.jlf.toml](https://github.com/PoOnesNerfect/jlf/blob/main/.jlf.toml).
You can copy this file into your config directory as `jlf/config.toml` or to your workspace as `.jlf.toml` or `jlf.toml`.

_**jlf.toml**_

```toml
# Default variables
# Replace or add variables as needed
[variables]
output    = "{#key timestamp|level|lvl|severity|message|msg|body|fields.message}{&timestamp}{&level}{&message}{#config compact} {:else}\\n{/config}{/key}{&data}"
timestamp = "{#key timestamp}{timestamp:dimmed} {/key}"
level     = "{#key level|lvl|severity}{level|lvl|severity:level} {/key}"
message   = "{message|msg|body|fields.message}"
data      = "{..:json}"
```

## Config File

Default config values are written in [PoOnesNerfect/jlf/.jlf.toml](https://github.com/PoOnesNerfect/jlf/blob/main/.jlf.toml).

Feel free to copy this into your config directory, like `$XDG_CONFIG_HOME/jlf/config.toml`, or your workspace directory as `.jlf.toml` or `jlf.toml`.

_**jlf.toml**_

```toml
# Default config values
[config]
format   = "{&output}"
compact  = false
no_color = false
strict   = false

# Default variables
[variables]
output    = "{#key timestamp|level|lvl|severity|message|msg|body|fields.message}{&timestamp}{&level}{&message}{#config compact} {:else}\\n{/config}{/key}{&data}"
timestamp = "{#key timestamp}{timestamp:dimmed} {/key}"
level     = "{#key level|lvl|severity}{level|lvl|severity:level} {/key}"
message   = "{message|msg|body|fields.message}"
data      = "{..:json}"
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

As we can see, our custom parser is about 3x faster than the `serde_json::Value` parsing.
Yes, it is still slower than the structured parsing, but our parser is still pretty darn fast for parsing a dynamic JSON data.
