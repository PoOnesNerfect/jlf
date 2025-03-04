# jlf

[![Crates.io][crates-badge]][crates-url]
[![MIT licensed][mit-badge]][mit-url]

[crates-badge]: https://img.shields.io/crates/v/jlf.svg
[crates-url]: https://crates.io/crates/jlf
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/PoOnesNerfect/jlf/blob/main/LICENSE

**jlf** is a CLI that converts hard-to-read JSON logs into colorful human-readable logs.

Simply pipe your JSON logs with `jlf`.

```sh
cat ./examples/dummy_logs | jlf
```

### Basic Example

**left**: `cat ./examples/dummy_logs`

**right**: `cat ./examples/dummy_logs | jlf`

<img width="1631" alt="Screenshot 2025-03-03 at 9 45 38 PM" src="https://github.com/user-attachments/assets/95b027e1-d005-48d7-a3c9-72b3b1be51b8" />

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
cargo install --path . --locked
```

## Table of Contents

<!--toc:start-->

- [jlf](#jlf)
  - [Basic Example](#basic-example)
  - [Installation](#installation)
    - [Cargo](#cargo)
    - [Manual Installation](#manual-installation)
  - [Table of Contents](#table-of-contents)
  - [CLI Options](#cli-options)
  - [Basic Usage](#basic-usage)
  - [Custom Formatting](#custom-formatting)
    - [Printing the entire JSON](#printing-the-entire-json)
    - [Styling](#styling)
    - [Available attributes](#available-attributes)
    - [Available Colors](#available-colors)
  - [Functions](#functions)
    - [log](#log)
    - [if](#if)
      - [else](#else)
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

Usage: jlf [OPTIONS] [FORMAT] [COMMAND]

Commands:
  expand  Print variable with its inner variables expanded. If no variable is specified, the default format string will be used
  list    List all variables
  help    Print this message or the help of the given subcommand(s)

Arguments:
  [FORMAT]  Formatter to use to format json log. [default: {&output}]

Options:
  -v, --variable <KEY=VALUE>  Pass variable as KEY=VALUE format; can be passed multiple times
  -n, --no-color              Disable color output. If output is not a terminal, this is always true
  -c, --compact               Display log in a compact format
  -s, --strict                If log line is not valid JSON, then report it and exit, instead of printing the line as is
  -t, --take <TAKE>           Take only the first N lines
  -h, --help                  Print help
  -V, --version               Print version
```

## Usage

### Compact Format

By default, **jlf** prints the standard log in the first line, then rest of json data in a pretty format in the following lines.

If you want to print everything in a single line, you can pass the option `-c`/`--compact`.

```sh
cat ./examples/dummy_logs | jlf -c
```

<img width="700" alt="Screenshot 2025-03-03 at 11 01 27 PM" src="https://github.com/user-attachments/assets/b6f9ebe3-1f51-4a5e-9127-5b55a5b0e0a6" />

### No Color

By default, **jlf** prints in pretty colors.

However, when you pipe logs into a file, **jlf** will automatically write with no colors, so the file isn't corrupted with ANSI characters.

For any reason, if you would like to print with no colors to the terminal, you can pass the option `-n`/`--no-color`.

```sh
# writing into a file will remove all ANSI characters automatically
cat ./examples/dummy_logs | jlf > pretty_logs

# pass `-n` to print to terminal with no colors
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
# Example Line: {"message": "User logged in successfully", "body": "My Body", "data": {"user_id": 3175, "session_id": "Nsb3P5mZ7971NFIt", "ip_address": "149.215.200.169", "friends":["Jack","Jill"]}}

# access the field by writing the field in braces
cat ./examples/dummy_logs | jlf 'Msg: {message}!' # -> Msg: User logged in successfully!

# if field may not exist, provide fallback fields separated by '|'. It will print the first field that exits.
cat ./examples/dummy_logs | jlf 'Msg: {msg|body|message}!' # -> Msg: My Body!

# access nested field using '.' as a separator.
cat ./examples/dummy_logs | jlf 'User {data.user_id} logged in!' # -> User 3175 logged in!

# access array items using '[n]' to index at `n`.
cat ./examples/dummy_logs | jlf 'My girl friend is {data.friends[1]}.' # -> My girl friend is Jill.

# if the field is an object or array, it will it as pretty json by default.
cat ./examples/dummy_logs | jlf 'user data: {data}'
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
cat ./examples/dummy_logs | jlf 'user({data.user_id}): {message}\n{.}'
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
cat ./examples/dummy_logs | jlf 'user({data.user_id}): {message}\n{..}'
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


### Conditionals

#### {#if cond1}{:else if cond2}{:else}{/if}
#### {#key field1}{:else key field2}{:else}{/key}
#### {#config config1}{:else}{/config}

### Variables

Supplied format above is the default format, so it will output the same as the default format.

`{#log}` is a function `log`, which is a convenience function that prints the basic log format.
Currently, function feature is very early and only `log` function is available.
`log` function is equivalent to the format `{timestamp:dimmed} {level|lvl:level} {message|msg|body}`.

`{#if spans|data}...{/if}` is a function `if`, which is a conditional function that prints the content inside the block if the field `spans` or `data` exists. More about the function's behavior in [{#if}](#if)

`\n` will print a newline.

`{spans|data:json}` will print the `spans` or `data` field as json.

`{timestamp:dimmed}` means that the cli will look for `timestamp` in the json and print it with `dimmed` dimmed.

`level|lvl` means that the cli will look for `level` and `lvl` in the json and use the first one it finds.
The style is also called `level`, which is a special style that will color the level based on the level (debug = green, info = cyan).

`{message|msg|body}` means that the cli will look for `message`, `msg`, and `body` in the json and use the first one it finds.

### Printing the entire JSON

If you want to print the entire JSON line, you can just use `{}`.

```sh
cat ./examples/dummy_logs | jlf '{}'
```

You can still provide styles to it.

```sh
cat ./examples/dummy_logs | jlf '{:compact,fg=green}'
```

What if you want to display either `spans` or `data` field, but if neither exists, just display the entire json?

```sh
cat ./examples/dummy_logs | jlf '{spans|data|:compact,fg=green}'
```

Notice the `|` at the end of `spans|data|`?
Empty string is interpreted as the entire json, so we're setting the fallback to printing the entire json.

### Styling

You can provide styles to the values by providing styles after the `:`.

```sh
cat ./examples/dummy_logs | jlf '{timestamp:bright blue,bg=red,bold} {level|lvl:level} {message|msg|body:fg=bright white}'
```

If you have multiple styles, you can separate them with `,`, like `fg=red,bg=blue`.

You can optionally provide the style type before the `=`. If you don't provide it, it will default to `fg`.

### Available attributes

- `dimmed`: make the text dimmed
- `bold`: make the text bold
- `fg={color}`: set the text color
- `{color}`: same as `fg={color}`
- `bg={color}`: set the background color
- `indent={n}`: indent the value by `n` spaces
- `key={color}`: sets the color of the key
- `value={color}`: sets the color of the value
- `str={color}`: sets the color of the string data type
- `syntax={color}`: sets the color of the syntax characters
- `json`: print the json value as json; this is the default and only available format, so you don't have to specify it
- `compact`: print in a single line
- `level`: color the level based on the level (debug = green, info = cyan)

`{color}` is a placeholder for any color value.

### Available Colors

You can view all available colors in [colors.md](https://github.com/PoOnesNerfect/jlf/blob/main/colors.md).

## Functions

Functions in jlf start with `#` inside `{}`.

### log

`log` is a convenience function that prints the basic log format.

```sh
cat ./examples/dummy_logs | jlf '{#log}'
```

equals

```sh
cat ./examples/dummy_logs | jlf '{timestamp:dimmed} {level|lvl:level} {message|msg|body|fields.message}'
```

and will print

```sh
2024-02-09T07:22:41.439284 DEBUG User logged in successfully
```

### if

You can use `{#if field}...{/if}` to conditionally print the content inside the block if the field exists.

The condition only checks if the field exists and is not null, but not the truthiness of the field.

If the field exists and the value is `false`, it will still print the content inside the block.

```sh
cat ./examples/dummy_logs | jlf '{#if spans|data}data: {spans|data:json}{/if}'
```

will print `data: { ... }` only if `spans` or `data` field exists.

#### else

Additionally, you can provide `{:else if field}` or `{:else}`

```sh
cat ./examples/dummy_logs | jlf '{#if spans|data}data: {spans|data:json}{:else if other.data}other: {fields}{:else}nothing here{/if}'
```

## Neat Trick

- If the line is not a JSON, it will just print the line as is.
- It removes all ANSI escape codes when piping to a file.

This means, you can just use `jlf` for non-JSON logs to pipe logs to a file without all the ansi escape codes.
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

```
```
