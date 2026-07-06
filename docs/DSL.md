# The jlf template DSL

This is the complete reference for jlf's `$`-based template language, with a
runnable example for every construct. A template turns one JSON record into one
line (or block) of output. The same language drives the default view, custom
layouts, and output formats like CSV/Markdown/HTML.

Every example below is a command you can paste. Output is shown after `# ->`;
where a record spans lines, the block is shown as-is. Piped output is uncolored,
so the examples read cleanly.

- [Mental model](#mental-model)
- [Fields](#fields)
- [Modifiers and styles](#modifiers-and-styles)
- [Conditionals](#conditionals)
- [Match](#match)
- [Repetition](#repetition)
- [Includes](#includes)
- [Output formats](#output-formats)
- [The default recipes](#the-default-recipes)
- [Cheatsheet](#cheatsheet)

## Mental model

Two families make up the whole language:

1. **Interpolation** — `${ … }` (and the bare `$name`) fills a hole with a field
   value. Everything inside `${ }` is a hole, never literal text. Literal text
   lives *outside* the braces; use `$$` for a literal `$`.
2. **Blocks** — the self-closing `$name( … )` forms: repetition (`$( )`,
   `$path( )`, `$cols( )`, `$rows( )`), conditionals (`$if`/`$has`/`$config`),
   and `$match`. Each closes on its balanced `)` — no end marker.

The block forms borrow their shape from Rust's `macro_rules!`: `$name`
interpolates like a metavariable, `$( … )sep*` repeats like macro repetition,
and `$match(subject $when(pat => …))` dispatches like a `match`.

## Fields

`$name` interpolates a top-level field. Use `${ … }` for anything more than a
bare identifier.

```sh
echo '{"level":"INFO"}' | jlf '$level'
# -> INFO
```

**Nested paths** use `.` (and `[i]` for array indices):

```sh
echo '{"user":{"id":7}}' | jlf '${user.id}'
# -> 7
```

**Fallback chains** with `|` — the first *present* field wins:

```sh
echo '{"msg":"hi"}' | jlf '${message|msg|body}'
# -> hi
```

**The whole record** is `${.}`, and **the rest** — every field not already
consumed by the template — is `${..}`. Objects and arrays render as JSON:

```sh
echo '{"a":1,"b":2}' | jlf -c '${.}'
# -> {"a":1,"b":2}

echo '{"a":1,"b":2}' | jlf -c 'a=$a rest=${..}'
# -> a=1 rest={"b":2}
```

`${..}` skips fields a template already showed (here `a`), which is how the
default view prints "the leftovers" without repeating what's on the line.

**Optional fields** are `${?field}`: when the value is empty or absent, one
adjacent space collapses so there's no stray gap.

```sh
echo '{"a":1}' | jlf 'x${?missing}y'
# -> xy
```

## Modifiers and styles

A modifier follows a `:` inside `${ … }`. Combine several with commas.

**Styling** — colors and attributes (ignored when output is piped or
`--no-color`):

| modifier | effect |
| -------- | ------ |
| `fg=NAME` / `bg=NAME` | foreground / background color (`fg=red`, `fg=#5fafff`, `bg=cyan`) |
| `dimmed`, `bold` | attributes |
| `red`, `cyan`, … | a bare color name is shorthand for `fg=NAME` |

```sh
echo '{"level":"INFO"}' | jlf '${level:fg=cyan,bold}'   # cyan bold on a terminal
```

**JSON rendering** for object/array values:

- `:json` — render as JSON (pretty by default; compact under `--compact`)
- `:indent=N` — indent width

```sh
echo '{"o":{"a":1}}' | jlf -c '${o:json}'
# -> {"a":1}
```

**Escape modifiers** make a value safe for a specific format. Use them on the
interpolation that needs it:

| modifier | escaping |
| -------- | -------- |
| `:csv` | CSV cell (quote when needed) |
| `:tsv` | TSV cell |
| `:md` | Markdown table cell |
| `:html` | HTML entities |
| `:none` | no escaping |

```sh
echo '{"v":"a,b"}' | jlf '${v:csv}'
# -> "a,b"

echo '{"v":"<x>"}' | jlf '${v:html}'
# -> &lt;x&gt;
```

## Conditionals

Conditionals are self-closing `$name( … )` blocks. The opening name decides how
the test works, and branches chain by adjacency — `$if(…)$elif(…)$else(…)`
written back-to-back (whitespace between adjacent branches is ignored, so you can
break them across lines). The body follows `=>`.

- `$if(X => body)` — **truthy** test. Empty strings, empty arrays/objects,
  `null`, a missing field, and `0` are false; everything else is true.
- `$if(X OP literal => body)` — **comparison**. `OP` is `==`, `!=`, `>`, `>=`,
  `<`, `<=`. Both sides compare numerically when they parse as numbers, otherwise
  as text; quote a string literal. A missing/non-scalar field never matches.
- `$has(FIELD => body)` — **existence** test. Unlike `$if`, a present-but-falsey
  value (`0`, `""`, `false`) still counts because the field is there.
- `$config(FLAG => body)` — branch on a config/CLI flag: `compact`, `no_color`,
  or `strict`.
- `$elif(COND => body)` and `$else(body)` continue a chain; `$else` is terminal.

```sh
printf '{"status":200}\n{"status":404}\n{"status":503}\n' \
  | jlf '$if(status >= 500 => down)$elif(status >= 400 => client)$else(ok)'
# -> ok
# -> client
# -> down
```

`$has` vs `$if` — the difference shows on a present-but-falsey value:

```sh
echo '{"body":0}' | jlf '$has(body => body=$body)$else(none)'
# -> body=0
echo '{"body":0}' | jlf '$if(body => yes)$else(no)'
# -> no
```

To render a fragment only when a field is present or truthy, use `$has`
(existence) or `$if` (truthy) with no else:

```sh
printf '{"span":{"m":"GET"}}\n{"x":1}\n' | jlf 'L$if(span.m => m=${span.m})'
# -> Lm=GET
# -> L
```

There is no `$elif`-has or `$elif`-config; nest instead, e.g.
`$else($has(x => …)$else(…))`.

Whitespace note: one space right after `=>` is a syntactic separator and is
dropped. An all-whitespace body (like `$else(\n)` or `$config(compact =>  )` with
two spaces) is kept as an intentional separator. Decorative line breaks and
indentation at the edges of a real body are dropped, so an arm can be written
across lines while its output stays on one line.

Because of that, a chain reads well multiline in a config recipe — each branch
on its own line, whitespace between them ignored:

```toml
[recipe.output]
out = '''$if(status >= 500 => ${timestamp} DOWN ${status})
$elif(status >= 400 => ${timestamp} WARN ${status})
$else(${timestamp} ok ${status})'''
```

## Match

`$match( subject  arms… )` dispatches on one scalar value; the first matching arm
renders. The subject is the first token (a path, fallbacks allowed) and is bound
to `$value` inside every arm body. A missing/`null` subject matches no arm — not
even `$else` — so the whole block renders nothing.

- `$when( PATTERN => body )` — an arm. The pattern/body split is the first
  top-level `=>` (outside quotes).
- `$else( body )` — the default arm for any present value.

Because a `$match` is self-closing on its `)`, the arms can go on their own lines
— whitespace between the subject and arms is ignored, so single-line and
multiline forms render identically (see the recipe below).

Patterns:

| pattern | matches |
| ------- | ------- |
| `>=500`, `== "GET"`, `!= 0` | a comparison against `$value` |
| `400..500`, `400..=499`, `500..`, `..500`, `..=3` | a numeric range (numeric only) |
| `200`, `"GET"` | a bare literal (equality) |
| `>=500 \| == 0`, `"GET" \| "HEAD"` | alternation — matches if any alternative does |

```sh
printf '{"s":200}\n{"s":404}\n{"s":503}\n' \
  | jlf '$match(s $when(>=500 => 5xx) $when(400..500 => 4xx) $else(ok:$value))'
# -> ok:200
# -> 4xx
# -> 5xx

echo '{"m":"HEAD"}' | jlf '$match(m $when("GET"|"HEAD" => read) $else(write))'
# -> read
```

As a reusable recipe (color HTTP status by class), referenced with `${@status}`:

```toml
[recipe.status]
out = '''$match(fields.status
  $when(>=500 => ${value:fg=red,bold})
  $when(>=400 => ${value:fg=yellow,bold})
  $else(${value:fg=green,bold})
)'''
```

## Repetition

Repetition has four sources. The identifier between `$` and `(` selects what is
iterated; omitting it means the current record. Inside the body, `$key` is the
name/index and `$value` is the value (use `${key}`/`${value}` when the next
character would otherwise extend the name, e.g. `[${key}]`).

`$( body )"sep"OP` — the record's top-level fields:

```sh
echo '{"a":1,"b":2}' | jlf '$( $key=$value )" "*'
# -> a=1 b=2
```

`$path( body )"sep"OP` — an object or array at `path`. Sub-paths of the value
work (`${value.name}`):

```sh
echo '{"f":{"x":1,"y":2}}' | jlf '$f( $key=$value )", "*'
# -> x=1, y=2

echo '{"xs":["a","b"]}' | jlf '$xs( [${key}] ${value} )", "*'
# -> [0] a, [1] b

echo '{"spans":[{"name":"a"},{"name":"b"}]}' | jlf '$spans( ${value.name} )" > "*'
# -> a > b
```

`$cols( body )"sep"OP` — the CLI-selected columns (a comma-list, `-f`/`--fields`,
or leftover bare words). Nothing iterates unless columns are selected:

```sh
echo '{"a":1,"b":2,"c":3}' | jlf '$cols( $value )","*' a,c
# -> 1,3
```

`$rows( body )"sep"OP` — the record stream. Text before `$rows(` prints once
before the first record, text after prints once after the stream, and the body
renders per record. This is how a format writes a header/footer:

```sh
printf '{"n":1}\n{"n":2}\n' | jlf 'head\n$rows( row ${n} )*footer'
# -> head
# -> row 1
# -> row 2
# -> footer
```

The operator `OP` is required: `*` (zero or more), `+` (one or more), `?` (zero
or one). The separator sits between `)` and the operator — bare (`$cols( $key ),*`)
or quoted when spaces matter (`$cols( $key )" | "*`). Whitespace just inside the
parens is trimmed, so `$( $key )` reads as `$key`. Each record's body is followed
by a newline automatically, so a per-row body needs no trailing `\n`.

Two conveniences for readable multiline bodies:

- A repetition (or a conditional block) on its own indented line absorbs the
  preceding line break, so the body renders on a fresh line per item.
- A field already shown via `${base.key}` is skipped by a later `$base( … )`
  (like `${..}`), so you can pull one field up and flatten the rest without
  duplication.

## Includes

`${@name}` inlines another recipe's `out`. A recipe whose `out` is a single field
or a column list is **optional by default** — an absent field collapses one
adjacent space — so you write `${@name}`, not `${?@name}`. The explicit
`${?@name}` optionalizes a template recipe.

```sh
printf '{"timestamp":"t","level":"INFO","message":"hi"}\n' \
  | jlf '${@timestamp}${@level}${@message}'
# -> t INFO hi
```

Inspect recipes with `jlf list` and expand one with `jlf expand NAME`.

## Output formats

An output format is just a recipe whose `out` frames the stream with `$rows(...)`
(or that sets a global `escape`). The built-ins `csv`, `tsv`, and `md` are seeded
from these definitions:

```toml
[recipe.csv]
out = "$cols( $key ),*\n$rows( $cols( ${value:csv} ),* )*"

[recipe.tsv]
out = "$cols( $key )\t*\n$rows( $cols( ${value:tsv} )\t* )*"

[recipe.md]
out = "| $cols( $key )\" | \"* |\n| $cols( --- )\" | \"* |\n$rows( | $cols( ${value:md} )\" | \"* | )*"
```

Run them by name with a column list:

```sh
printf '{"a":"1","b":"x,y"}\n{"a":"2","b":"z"}\n' | jlf @csv a,b
# -> a,b
# -> 1,"x,y"
# -> 2,z

printf '{"a":"1","b":"2"}\n' | jlf @md a,b
# -> | a | b |
# -> | --- | --- |
# -> | 1 | 2 |
```

Custom formats are recipes too. An HTML report frames the stream once and escapes
each value:

```toml
[recipe.report]
escape = "html"
out = "<ul>\n$rows( <li>${level}: ${msg}</li>\n)*</ul>\n"
```

```sh
printf '{"level":"INFO","msg":"a<b"}\n{"level":"ERR","msg":"boom"}\n' | jlf @report
# -> <ul>
# -> <li>INFO: a&lt;b</li>
# -> <li>ERR: boom</li>
# -> </ul>
```

## The default recipes

jlf ships five recipes that make up the default view. `@output` is the entry
point; it includes the others. Each is optional by default, so an absent field
collapses.

```toml
[recipe.output]
out = "${@timestamp} ${@level} ${@message}\n${@data}"

[recipe.timestamp]
out = "timestamp:dimmed"

[recipe.level]
out = '''$match(lvl|level|severity
  $when("ERROR"|"error" => ${value:fg=red})
  $when("WARN"|"warn"   => ${value:fg=yellow})
  $when("INFO"|"info"   => ${value:fg=cyan})
  $when("DEBUG"|"debug" => ${value:fg=green})
  $when("TRACE"|"trace" => ${value:fg=cyan,dimmed})
  $else(${value})
)'''

[recipe.message]
out = "message|msg|body|fields.message"

[recipe.data]
out = "..:json"
```

| recipe | what it does |
| ------ | ------------ |
| `output` | timestamp, level, message, then the rest as JSON on the next line (same line when `--compact`) |
| `timestamp` | the `timestamp` field, dimmed |
| `level` | dispatches on `lvl\|level\|severity` and colors it by severity; unknown → uncolored, missing → nothing |
| `message` | the first present of `message\|msg\|body\|fields.message` |
| `data` | `${..}` — every field not already shown, as JSON |

```sh
printf '{"timestamp":"t1","level":"WARN","message":"slow","code":5}\n{"note":"raw"}\n' | jlf
# -> t1 WARN slow
# -> {
# ->   "code": 5
# -> }
# -> {
# ->   "note": "raw"
# -> }
```

The second record has no timestamp/level/message, so those collapse and only
`@data` prints. `--compact` keeps the JSON inline:

```sh
printf '{"timestamp":"t1","level":"WARN","message":"slow","code":5}\n' | jlf -c
# -> t1 WARN slow {"code":5}
```

The built-in default (used when no config is present) is equivalent, choosing the
separator inline: `${@timestamp}${@level}${@message}$config(compact =>  )$else(\n)${@data}`.

## Cheatsheet

| form | meaning |
| ---- | ------- |
| `$name`, `${a.b.c}` | a field, nested by `.` |
| `${a\|b\|c}` | fallback chain; first present wins |
| `${field:mod}` | modifiers: styling (`fg=`, `dimmed`), `json`, escaping (`csv`/`html`/…) |
| `${.}` / `${..}` | whole record / the rest (fields not yet shown) |
| `${?field}` | optional; if empty, one adjacent space collapses |
| `$if(COND => …)` | truthy, or comparison when `COND` has `== != > >= < <=` |
| `$elif(COND => …)` / `$else(…)` | continue a condition chain (adjacency) |
| `$has(FIELD => …)` | existence test (present-but-falsey counts) |
| `$config(FLAG => …)` | branch on `compact` / `no_color` / `strict` |
| `$match(subj $when(pat => …) $else(…))` | value dispatch; binds `$value`; first arm wins |
| `$( … )"sep"*` | repeat over the record's top-level fields |
| `$path( … )"sep"*` | repeat over an object/array at `path` |
| `$cols( … )"sep"*` | repeat over CLI-selected columns |
| `$rows( … )"sep"*` | repeat over the record stream (text before/after prints once) |
| `${@name}` / `${?@name}` | include another recipe's body (optional form) |
| `$$` | a literal `$` |

For the recipe/config mechanics around these templates — keys, filters,
summaries, conditional overrides, named fields — see the "Recipes and configuration" section of the [README](../README.md).
