# Recipes — one named, reusable thing

Status: **draft / design** (not yet implemented). This supersedes the separate
notions of _variables_, _templates_, _presets_, and _custom formats_ with a
single concept.

## Why

jlf grew four overlapping names for "a saved way to look at logs":

- **variable** — a named fragment of a layout (`{&level}`)
- **template** — a layout string (`{ts} {msg}`)
- **preset** — a saved command (filters + layout + summary)
- **format** — a custom output (header/row/footer/escape)

But a variable _is_ a template (a named one); a preset _is_ a template plus
filters; a format _is_ a template plus framing. One idea, four words, and two
sigils (`{&…}` vs `@…`). Recipes collapse them into **one concept, one sigil
(`@`)**.

## The concept

A **recipe** is a named, reusable definition. Refer to it as `@name`:

- `jlf @name` — **run** it (filter → summarize/render → frame).
- `{@name}` — **inline** it inside another layout (its `body` only).

Recipes live in config and compose freely. The default per-record layout is the
`@output` recipe.

### Config form

```toml
# Shorthand: body-only recipes (this replaces [variables])
[recipes]
compact = "{ts} {msg}"

# Full form: any recipe with more than a body (replaces [preset.*] and [format.*])
[recipe.errors]
filter = "lvl|level|severity=error,fatal"
body   = "{ts} {level} {msg}"
```

`@name` is the _usage_ sigil; the TOML sections are `[recipe.NAME]` / `[recipes]`
(TOML bare keys can't start with `@`). This mirrors today's `[preset.errors]` →
`@errors`.

### Keys (all optional)

| key                                | purpose                                                                                                                           |
| ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `body`                             | per-record layout (a format string; may contain `{field}`, `{@other}`, conditionals)                                              |
| `filter`                           | keep matching records — `key=value`, `\|` = fallback field, `,` = OR of values, space = AND                                       |
| `field`                            | value accessor (`a\|b.c`) — makes the recipe usable _as a value_                                                                  |
| `style`                            | render modifier(s) for `field` (`level`, `dimmed`, `red`, `json`, …) — maps to the `:mod` syntax                                  |
| `fields`                           | comma list; shorthand for `body = "{a} {b} {c}"`                                                                                  |
| `header` / `footer`                | emitted once, before / after all records                                                                                          |
| `escape`                           | escape special characters in interpolated values so they're safe for the output (`html` → `<` becomes `&lt;`, etc.; default none) |
| `count` / `stats` / `top` / `uniq` | run a summary over a field (a name or `@fieldrecipe`)                                                                             |
| `by`                               | group field for the summary                                                                                                       |
| `n`                                | N for `top`                                                                                                                       |
| `compact`                          | force compact rendering                                                                                                           |
| `base`                             | inherit another recipe (`@other`), then override its keys                                                                         |

## Conditional overrides

A sub-table `[recipe.NAME.<condition>]` overrides individual keys of
`[recipe.NAME]` when the condition holds. This keeps conditionals out of the
template strings — you state only what _changes_, with no duplicated definition:

```toml
[recipe.data]
field = ".."
style = "json"

[recipe.data.compact]      # when the `compact` flag is set, override style
style = "compact-json"
```

Conditions are the same vocabulary as filters — a config flag (`compact`), a
field test (`level=error`), or rest-state — and overrides are shallow (only the
named keys change; the rest of the base recipe stands). This replaces the old
inline `{#config compact}{:else}{/config}` branch with structure.

## Two ways to use `@name`

### Run — `jlf @name`

The full pipeline, in order:

1. **filter** — `filter` records (merged under any explicit CLI filters).
2. **summarize** — if the recipe sets a summary verb (and no explicit
   subcommand), run it and stop.
3. **render** — otherwise render each record with `body` (or `field`+`style`, or
   `fields`), wrapped in `header`/`footer`, with `escape` applied.

Explicit CLI args layer on top: a same-field filter overrides, a different-field
filter is added, and an explicit template/fields/format/flag wins.

```sh
jlf @errors                  # filter + render with body
jlf @errors status=500       # add a filter
jlf @errors '{ts} {msg}'     # override the body
jlf -p latency-report        # a summary recipe → grouped stats
```

### Inline — `{@name}`

Inside any `body`, `{@name}` substitutes **only that recipe's `body`** (or, if it
has no `body`, its `field`+`style` rendering). Everything else — `filter`,
`header`/`footer`, `escape`, summaries — is invocation-level and ignored when
inlined.

- `'{@level} {msg}'` → inline the level layout.
- `'{@errors} hello'` → inlines `@errors.body` only (`{ts} {level} {msg} hello`);
  the `filter` is **not** applied.
- `'{@report}'` → just the row layout; **no** header/footer/escape.
- `{@actions}` where `@actions` is summary-only → renders nothing (warn: "recipe
  @actions has no body to inline").

Nested `{@…}` expand recursively; reference cycles are detected and reported.

> One name, two contexts: **run** uses the whole recipe, **`{@…}`** uses its body.

## Named fields: define a value once

A recipe with a `field` (and optional `style`) names a _value_, reusable across
rendering, filtering, and summarizing. This is for fields you refer to a lot or
that have several aliases — not something you're forced to do for simple ones.

```toml
[recipe.latency]
field = "latency_ms|duration|elapsed"
```

```sh
jlf filter @latency>500     # filter: on latency_ms|duration|elapsed
jlf stats @latency          # summarize: by that field
jlf '{@latency}ms {msg}'    # render: its value
```

This is the "define your schema once" payoff: your logs call the same thing
several names, but you only say so in one place. You can still inline a field
directly (`{lvl|level|severity:level}`) for a quick one-off — naming it is for
when you'll reuse it (which the default config does; see below).

## `|` fallback in filters

Filters gain the same `|` fallback as templates, so the rule ("`|` = first
present field") is identical everywhere:

```sh
jlf 'lvl|level|severity=error'   # match whichever of those keys exists
```

`a|b=x` is true when any of `a`, `b` equals a listed value; `,` still ORs values
and multiple filters AND together.

## What the defaults become

Today's defaults use a manual `{#key …}{/key}` guard around every field just to
manage spacing:

```toml
[variables]
output    = "{#key timestamp|level|lvl|severity|message|msg|body|fields.message}{&timestamp}{&level}{&message}{#config compact} {:else}\n{/config}{/key}{&data}"
timestamp = "{#key timestamp}{timestamp:dimmed} {/key}"
level     = "{#key level|lvl|severity}{level|lvl|severity:level} {/key}"
message   = "{message|msg|body|fields.message}"
data      = "{..:json}"
```

Simple fields become tiny `field` recipes, the rest-as-JSON becomes a `@data`
field recipe, and `@output` just joins them with `{?@…}`. The compact-vs-newline
separator is a one-line conditional override on `@output` — **no inline
conditionals at all**:

```toml
[recipe.output]                          # default: pieces joined, newline before the JSON
body = "{?@timestamp} {?@level} {?@message}\n{?@data}"

[recipe.output.compact]                  # when --compact: a space before the JSON instead
body = "{?@timestamp} {?@level} {?@message} {?@data}"

[recipe.timestamp]
field = "timestamp"
style = "dimmed"

[recipe.level]
field = "lvl|level|severity"
style = "level"

[recipe.message]
field = "message|msg|body|fields.message"

[recipe.data]
field = ".."
style = "json"
```

How it stays clean:

- `{?@timestamp}` / `{?@level}` / `{?@message}` drop their trailing space when the
  field is absent.
- `{?@data}` renders **empty when no fields are left over**, and its `?` collapses
  the adjacent separator — so a fully-consumed record prints just the log line,
  with no trailing blank line. This replaces the old `{#if ..}` guard.
- The compact separator is the single override line, replacing
  `{#config compact} {:else}\n{/config}`.

Each field is now reusable on its own (`jlf filter @level=error`,
`jlf stats @latency`). The config reads top-down: the high-level `@output` entry
point first, then the lower-level pieces it's composed from.

This relies on two render rules (see Open questions): an optional rest
(`{?@data}` / `{?..}`) is _empty_ when there are no leftover fields, and the
`?`-collapse absorbs an adjacent newline as well as spaces.

## Migration (nothing breaks)

For at least one release, the old names are deprecated **aliases**:

| old                               | new                                                                        |
| --------------------------------- | -------------------------------------------------------------------------- |
| `[variables]`                     | `[recipes]`                                                                |
| `[preset.NAME]` / `@name`         | `[recipe.NAME]` / `@name` (unchanged usage)                                |
| `[format.NAME]` / `--format NAME` | `[recipe.NAME]` (with header/body/footer/escape); run with `@name` or `-p` |
| `{&name}`                         | `{@name}`                                                                  |
| `[config] format = "…"`           | the `@output` recipe                                                       |
| filter key `where`                | `filter`                                                                   |

(`escape` is unchanged; `base` is new — no alias needed.)

## Phased implementation

1. **Safe, additive:** `{&}`→`{@}` (alias), `|` fallback in filters.
2. **Rename + alias:** `[variables]`→`[recipes]`; `where`→`filter`.
3. **Unify:** one `[recipe.*]` resolver merging today's preset + format paths
   (filter/body/header/footer/escape/summary/base); `@name` run vs `{@name}`
   inline; conditional-override sub-tables `[recipe.NAME.<cond>]`.
4. **Named fields:** `field`/`style`, usable in render + filter + summarize.
5. **Render rules** the clean default needs: optional rest (`{?..}`/`{?@data}`)
   is empty when no leftover fields; `?`-collapse absorbs an adjacent newline.
6. Switch the default config to the recipe form; verify output byte-for-byte
   (modulo the deliberately-dropped data-only-line edge).

## Resolved

- Noun: **recipe**. Layout key: **body**. Filter key: **filter**. Inherit:
  **base**. Escape key: **escape** (kept; clearly documented).
- One sigil **@**; `{&}` aliased. `?` is a **prefix** (`{?@data}`, `{?level}`) —
  unambiguous with `:modifiers`, consistent with the other front-anchored sigils.
- Conditionals move onto recipes via `[recipe.NAME.<cond>]` overrides; simple
  fields inline.

## Open questions

- `style` key name — it covers colors _and_ `json`/`compact`; is `style` right,
  or `as` / `render`?
- If a recipe has both `field` and `body`, `body` wins for rendering and `field`
  is used for filter/summarize — confirm.
- Should `{@name}` be allowed to carry `escape` (escape its inlined body), or is
  `escape` strictly output-level? (Current: output-level only.)
- Do we keep a short alias for "the default output" (e.g. `jlf` with no recipe ==
  `@output`)?
- Invocation flag: keep `-p`/`--preset`, or rename it (e.g. `--use NAME`) now that
  it runs any recipe, not just a "preset"? (`@name` works regardless.)
- Override conditions: which vocabulary exactly (config flags, field tests,
  rest-state) and how `[recipe.NAME.<cond>]` keys spell them.
