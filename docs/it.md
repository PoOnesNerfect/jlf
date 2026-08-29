# The jlf command builder (`jlf it`)

`jlf it` is an interactive builder for `jlf` commands. You pick what you want to
build, then assemble it — filters, fields, a summary, a table — while a live
preview of the result updates on every keystroke against a sample of your data.
When you're happy you can run the command against the real input or save it as a
recipe.

It's a way to discover fields and get a command right without memorizing the
`$`-template DSL or the exact flag names; the command it builds is an ordinary
`jlf` invocation you could also have typed by hand.

- [Running it](#running-it)
- [Choosing what to build](#choosing-what-to-build)
- [The edit menu](#the-edit-menu)
- [The live preview](#the-live-preview)
- [Editing a field with autocomplete](#editing-a-field-with-autocomplete)
- [Filters](#filters)
- [Run it](#run-it)
- [Saving a recipe](#saving-a-recipe)
- [Where the sample comes from](#where-the-sample-comes-from)

## Running it

`jlf it` and `jlf-it` are the same program (the former dispatches to the latter,
git-style):

```sh
jlf it app.log                 # build against a file
head -200 app.log | jlf it     # ...or a finite pipe (used as the sample)
docker logs -f web | jlf it    # ...or a live stream (sampled, then kept open)
jlf it                         # ...or pick a sample interactively
```

## Choosing what to build

The first prompt picks a mode, which decides what the rest of the builder offers:

- **View** — pretty-print records, choose which fields to show, toggle compact,
  redact.
- **Summarize** — `count` / `stats` / `top` / `uniq` over a field, optionally
  grouped, with an output format.
- **Export** — a `csv` / `tsv` / `md` table of selected columns.

You can switch modes later from the menu (**Change mode**, accelerator `m`).

## The edit menu

After choosing a mode you land on the main menu. Each row shows one part of the
command and its current value, plus a one-key **accelerator** to jump straight to
it. Press the accelerator (or move with `↑`/`↓` / `j`/`k` and press `Enter`) to
edit that part.

| accelerator | edits |
| --- | ------ |
| `f` | filters |
| `t` | fields to show (View) or the table columns (Export) |
| `c` | compact on / off (View) |
| `d` | redaction (View) |
| `y` | the summary — verb, field, group-by (Summarize) |
| `o` | the output format (Summarize) |
| `m` | change mode |
| `r` | **Run it** |
| `s` | **Save** as a recipe |
| `q` | quit |

The menu also shows the equivalent `jlf` command as you build, so you can learn
the flags by watching them change.

## The live preview

Two bordered panels sit above the menu: the **sample record** (colored, with the
fields you're editing highlighted) and a **preview** of your command's output
against the sample. Editing any part refreshes the preview on every keystroke.

The preview is curated so you see variety rather than repetition: near-identical
records collapse and error records float to the top. When a filter excludes every
sampled record, the preview doesn't just go blank — it synthesizes a matching
record (by adjusting a real sample line to satisfy the filter) and shows that,
clearly labelled, so you can still see the shape of the result. While you edit a
View filter, non-matching records stay on screen dimmed rather than vanishing.

If a record is taller than its panel, `PgUp` / `PgDn` scroll the sample so you can
read all of it.

## Editing a field with autocomplete

Inside a field editor (filters, fields, columns, a summary field), the builder
suggests the sample's field paths — nested and array paths included, like
`fields.status` or `spans.0.method` — then the comparison operators, then that
field's actual values.

Nothing is selected until you press **Tab** / **↓**, which selects and fills
successive candidates so **Enter** applies immediately. **Shift-Tab** / **↑**
steps back and, past the first item, restores exactly what you typed. Editing the
text deselects. **Esc** leaves the field. Standard line editing works too:
**Ctrl-W** deletes a word (stopping at `.` and `,` inside a path), plus
**Ctrl-U** / **Ctrl-K** / **Ctrl-A** / **Ctrl-E**.

## Filters

The filter editor takes the same syntax as the `jlf` CLI and the viewer:
`field op value` tokens with operators `=`, `!=`, `>`, `>=`, `<`, `<=`, `~`, `!~`,
space-separated. A value may contain spaces — the whole token after the operator
is the value, so `fields.message~Build mode: DEBUG` is a single filter. Leave the
field blank to clear all filters.

## Run it

**Run it** (`r`) executes the command you've built against the *real* input, not
the finite sample: it re-reads a file, or resumes a live pipe, so a command built
from `docker logs -f | jlf it` keeps streaming instead of stopping at the sample.

## Saving a recipe

**Save** (`s`) writes the built command as a `[recipe.<name>]` entry in your
workspace config (`.jlf.toml` / `jlf.toml`). You can then rerun it with
`jlf @<name>` or open it in the viewer. See [DSL.md](DSL.md) for the recipe
format.

## Where the sample comes from

The builder needs a finite sample to preview against. It takes one from, in
order: an explicit file argument, piped stdin, or an interactive picker.

For a piped stream the sample is read without blocking: a finite pipe is read to
its end, while a live stream that emits a burst and then goes quiet (a server log,
`docker logs -f`, …) is sampled as soon as it idles — it does not have to close
first. Only a stream that produces nothing at all gives up, with a hint to pass a
finite sample instead:

```sh
jlf it app.log                 # a file
head -100 app.log | jlf it     # a bounded slice of a stream
```

If a live producer buffers its stdout when piped (so no data arrives promptly),
make it line-buffered on its side — e.g. `stdbuf -oL producer | jlf it`.
