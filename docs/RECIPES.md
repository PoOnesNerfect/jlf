# Recipes — one named, reusable thing

Status: implemented. Recipes are the named form for saved commands, reusable
layout fragments, named fields, and output formats.

## Why

jlf used to have several overlapping names for "a saved way to look at logs":
variables, templates, presets, and custom formats. A recipe is the shared shape
for those cases:

- a reusable layout fragment, such as `${ @level }`
- a template, such as `$ts $msg`
- a saved command, such as filters plus a layout or summary
- an output format, such as a CSV or HTML table

`@name` runs a recipe from the command line. An include directive inside a
layout inlines a recipe's `body`.

## The concept

A recipe is a named, reusable definition. Refer to it as `@name`:

- `jlf @name` runs it (filter → summarize/render → `body` template).
- `${ @name }` inlines its `body` inside another layout.

Recipes live in config and compose freely. The default per-record layout is the
`@output` recipe.

## Config form

```toml
# Shorthand: body-only recipes
[recipes]
compact = "$ts $msg"

# Full form: any recipe with more than a body
[recipe.errors]
filter = "lvl|level|severity=error,fatal"
body   = "$ts $level $msg"
```

`@name` is the usage sigil; TOML sections are `[recipe.NAME]` or `[recipes]`
because TOML bare keys cannot start with `@`.

## Keys

All keys are optional. A recipe usually uses only the keys needed for its role.

| key | purpose |
| --- | ------- |
| `body` | per-record layout; may contain fields, directives, repetitions, and `${ @other }` includes |
| `filter` | keep matching records — `key=value`, `\|` fallback fields, `,` OR values, spaces AND filters |
| `field` | value accessor (`a\|b.c`) that makes the recipe usable as a named value |
| `style` | render modifier(s) for `field` (`level`, `dimmed`, `red`, `json`, etc.) |
| `fields` | comma list; shorthand for a `$a $b $c` body and the columns used by `$cols(...)` |
| `escape` | default escape modifier for interpolated values in the recipe (`html`, `csv`, `tsv`, `md`, or `none`); also marks the recipe as a format |
| `count` / `stats` / `top` / `uniq` | run a summary over a field or named field recipe |
| `by` | group field for a summary |
| `n` | N for `top` |
| `compact` | force compact rendering |
| `format` | render through another output format recipe (`csv`/`tsv`/`md` or custom) |
| `base` | inherit another recipe (`@other`), then override its keys |

## The `$` template DSL

Plain text is literal. `$` introduces interpolation. `{` and `}` are ordinary
literal characters unless they are part of `${ … }`. Use `$$` for a literal
`$`.

| form | meaning |
| ---- | ------- |
| `$name` | bare field; simple identifiers only |
| `${user.id}` | field path |
| `${a|b|c}` | fallback chain; first present field wins |
| `${ts:dimmed}` | field with a modifier |
| `${lvl|level:level}` | fallback chain with a modifier |
| `${.}` | whole record |
| `${..}` / `${..:json}` | fields not already consumed by the template |
| `${ ?field }` | optional field; if empty, one adjacent space collapses |
| `${if COND}` / `${key COND}` / `${config FLAG}` | start a conditional block |
| `${else}` / `${else if COND}` / `${else key COND}` | conditional branch |
| `${/}` | end any conditional block |
| `${ @name }` | include another recipe's `body` |
| `${ ?@name }` | optional include |

The spaced optional/include forms above are equivalent to the compact spelling;
the spaces are shown to make the sigils easier to scan in prose.

## Repetition

Repetition has four sources. The identifier between `$` and `(` selects what is
iterated; omitting the identifier means the current record itself.

`$( body )"join"OP` repeats over the current record's top-level fields. Inside
the body, `$key` is the field name and `$value` is the field value.

`$path( body )"join"OP` repeats over an object or array at `path` in the current
record. Inside the body, `$key` is the object key or array index and `$value` is
the entry value. Sub-paths of the value work, such as
`$spans( ${value.name} )", "*`.

`$cols( body )"join"OP` repeats over the CLI-selected columns from a comma-list,
`-f`/`--fields`, or leftover bare words. Inside the body, `$key` is the column
name and `$value` is the column value. If no columns are selected, there is
nothing for `$cols(...)` to iterate.

`$rows( body )"join"OP` repeats over the record stream. Text before `$rows(` is
written once before the first record, text after it is written once after the
stream, and the body is rendered once per record.

For all repetition forms, `OP` is required:

- `*` — zero or more
- `+` — one or more
- `?` — zero or one

The join text sits between `)` and the operator. It can be bare, as in
`$cols( $key ),*`, or quoted when spaces should be visible, as in
`$cols( $key )" | "*`. No join text is written as `$cols( $key )*`. Whitespace
just inside `$( … )` and the named repetition forms is trimmed, so
`$cols( $key )` reads as `$key`.

```toml
[recipe.fields]
body = "$fields( $key=$value )\" \"*"
```

Array fields use the same form:

```toml
[recipe.errors]
body = "$errors( [$key] $value )\", \"*"
```

## Escape modifiers

Escaping is a value modifier. Use it on the interpolation that needs it:

- `:html` — HTML-escape a value
- `:csv` — CSV cell escaping
- `:tsv` — TSV cell escaping
- `:md` — Markdown table cell escaping
- `:none` — no escaping

A recipe may also set a global `escape = "html"` (or `csv`, `tsv`, `md`,
`none`) to apply that default to all of its interpolations. Per-cell modifiers
still make the escaping visible where it matters, especially in output-format
recipes.

## Output formats are recipes

A `[recipe.NAME]` is treated as an output format when it has `escape` or its
single `body` template uses `$rows(...)` or `$cols(...)`. `$rows(...)` is the
stream-framing mechanism: anything before it is written once before the first
record, the body inside it is written once per record, and anything after it is
written once after the stream.

The built-in `csv`, `tsv`, and `md` formats are seeded from these definitions:

```toml
[recipe.csv]
body = "$cols( $key ),*\n$rows( $cols( ${value:csv} ),* )*"

[recipe.tsv]
body = "$cols( $key )\t*\n$rows( $cols( ${value:tsv} )\t* )*"

[recipe.md]
body = "| $cols( $key )\" | \"* |\n| $cols( --- )\" | \"* |\n$rows( | $cols( ${value:md} )\" | \"* | )*"
```

Run them like any recipe:

```sh
jlf @csv ts,level
jlf @md ts,level
jlf count level @csv
```

Columns come from a comma-list, `-f`/`--fields`, or leftover bare words. A table
format needs columns because its `$cols(...)` repetitions iterate that list.

Custom dialects are recipes too:

```toml
[recipe.psv]
body = "$cols( $key )\"|\"*\n$rows( $cols( ${value:csv} )\"|\"* )*"
```

## Dynamic HTML tables

Because output formats are ordinary recipes, HTML reports can be framed once
with `$rows(...)`:

```toml
[recipe.report]
escape = "html"
body = "<table>\n$rows( <tr><td>${level}</td><td>${msg}</td></tr> )*</table>\n"
```

Dynamic-column tables use `$cols(...)` inside the per-record `$rows(...)` body:

```toml
[recipe.htable]
body = "<table>\n$rows( <tr>$cols( <td>${value:html}</td> )*</tr> )*</table>"
```

Use it with a column list:

```sh
jlf @htable ts,level,message
jlf @htable -f ts,level,message
```

## Flattening an object inline

Object iteration is useful for trace-style logs where one nested object contains
most of the useful context:

```toml
[recipe.output]
body = "${?timestamp:dimmed} ${?level:level} ${?target:dimmed}  $fields( $key=${value:dimmed} )\" \"*"
```

For a record with `fields = {"message":"ok","dir":"/d","n":5}`, this flattens
the nested entries as `message=ok dir=/d n=5`.

## Conditional overrides

A sub-table `[recipe.NAME.<condition>]` overrides individual keys of
`[recipe.NAME]` when a config flag holds. The supported flags are `compact`,
`no_color`, and `strict`. Overrides are shallow: only the named keys change.

```toml
[recipe.output]
body = "${@timestamp} ${@level} ${@message}\n${@data}"

[recipe.output.compact]
body = "${@timestamp} ${@level} ${@message} ${@data}"
```

Use per-record directives inside `body` when a branch depends on record content.

## Named fields

A recipe with `field` and optional `style` names a value that can be reused in
rendering, filtering, and summaries.

```toml
[recipe.latency]
field = "latency_ms|duration|elapsed"
```

```sh
jlf filter @latency>500
jlf stats @latency
jlf '${ @latency }ms $msg'
```

You can still write the field directly for a one-off:
`${latency_ms|duration|elapsed}`. Naming it is for values you reuse.
