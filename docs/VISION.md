# jlf — Vision & Strategy

> Status: draft / working reference. A direction, not a commitment.
>
> Parts of this are now implemented: filters, summaries, redaction, export, the
> workspace/plugin split, `jlf-tui`, and the **recipe** model — one `@name`
> concept for variables, saved commands, and output formats (what this doc calls
> "presets" and format flags). Output formats (`csv`/`tsv`/`md` and custom
> formats) are recipes run with `@name`; see the README (Recipes and configuration) for the shipped
> design. This vision doc keeps the original framing and the still-unbuilt parts.

## 1. Summary

jlf today turns JSON logs into readable, formatted output using a template string
DSL. The direction proposed here keeps that core and extends it into a general
tool for working with structured data streams on the command line: viewing,
filtering, reshaping, redacting, exporting, and summarizing JSON / NDJSON (and,
later, other structured formats).

The guiding constraint is to add capability without adding syntax users have to
learn. The output template is already familiar (it reads like string
interpolation); filtering, summarizing, and exporting should reuse mental models
people already have rather than introduce a query language.

## 2. The gap

Working with structured logs on the command line today means juggling several
tools, each with its own syntax: grep/cut are structure-blind, jq/awk are their own
languages, and the niche tools cover one slice each — `hl`/`humanlog`/`fblog`
format, `lnav` browses, `angle-grinder` aggregates, `mlr` (Miller) does stats,
`gron` flattens for grep. The opportunity is one structure-aware CLI that covers the
common cases — read legibly, filter to a request, redact, convert, quick
count/percentile — behind a single small surface.

This is a local CLI, not a platform. It does not index or store data, so it does
not compete with Splunk/Datadog/Loki for historical, multi-host, dashboard-scale
analytics — that is a database problem, and out of scope. Concretely, jlf is for:

1. A drop-in for grep/jq/cut/sed when working with structured logs.
2. Filter / redact / convert (NDJSON → CSV/table, scrub secrets).
3. A daily pretty viewer — readable, colored, level-aware.
4. A pre-ship pipeline stage — filter/redact/reshape before logs hit a backend.
5. Light ad-hoc analytics (count/stats/top) on one stream or file, no backend.

Everything is single-box, streaming or small-file, zero-setup.

## 3. Design principles

1. **Reuse existing mental models.** String templates for output, `key=value`
   for filtering, plain English words for summaries.
2. **Defaults over configuration.** `stats latency` prints a standard summary;
   the user doesn't choose aggregates.
3. **Auto-detect over declaration.** Sniff the input format where practical.
4. **One engine, one or two front-ends.** The same core runs as the CLI and,
   later, a local server; reused as an embeddable library.
5. **Light core, optional extensions.** Heavy features ship as separate
   binaries discovered on `PATH`, not built into the core.

These are constraints to hold features to, not selling points.

## 4. The surface (UX)

### Telling arguments apart (CLI grammar)

A subcommand, if used, is always the **first token** (git-style): `jlf count …`,
`jlf serve`, `jlf tui`. Anything else is the default "view" command, whose
arguments are classified by shape. Because verbs are only recognized in first
position, nothing later in the line collides with one. The kinds:

| Kind       | Example                             | Recognized by        |
| ---------- | ----------------------------------- | -------------------- |
| subcommand | `count`, `stats`, `serve`           | first token, bare    |
| flag       | `-f ts,level`, `-i app.log`, `-c`   | starts with `-`      |
| recipe     | `@errors`, `@csv`                   | starts with `@`      |
| template   | `'$ts $level'`                      | contains `$`         |
| filter     | `level=error`                       | contains an operator |

Fields and input are flags, not positionals: `-f`/`--fields` selects fields,
`-i`/`--input` names a file. Classification:

1. A bare first token is the command: a verb (`count` `stats` `top` `uniq`) runs
   that summary; an installed `jlf-<word>` is an extension (`serve`, `tui`);
   neither → the default view command. The rest of the line follows.
2. `-…` is a flag; `@…` a recipe; `$` → template; a comparison operator
   (`=` `!=` `>` `<` `>=` `<=` `~` `!~`) → filter.

```sh
jlf '$ts $level'                # template -> formatting
jlf level=error                 # operator -> filtering
jlf -f ts,level                 # --fields -> show these fields
jlf count level                 # subcommand (first) -> summary
jlf @errors                     # recipe
jlf -i app.log level=error      # --input file + filter
```

Roles compose, and verbs stay first:

```sh
jlf '$ts $msg' level=error            # filter + format
jlf count endpoint level=error          # summarize + filter
jlf -i app.log level=error -f ts,level  # input + filter + fields
```

Because no verb appears after the first token, an output-format recipe may take its columns
inline — `jlf count latency @md ts,p99` — with `-f`/`--fields` as the explicit
alias. For scripts, the explicit flags are fully deterministic — `--where`,
`--fields`, `--input`. The positional shapes (template, filter) are shorthand.

### Input sources

Input is read from stdin or named files; there are no bare-word filenames, so a
file never collides with a command.

- **stdin (default):** `cat app.log | jlf …`, `jlf … < app.log`.
- **`-i` / `--input`:** `jlf -i app.log …`, repeatable for multiple files read in
  order as one stream.
- **Following a live source** is the upstream tool's job, piped in: `tail -f app.log | jlf …`, `kubectl logs -f | jlf …`. jlf reads stdin as lines arrive, so
  there is no separate follow flag.

### Format, project, and export — one mechanism

The template is both the field projection and the output format:

```sh
jlf '$ts $level'                    # pick fields, pretty output
jlf '${ts},${level},${latency_ms}'   # the same mechanism can produce delimited text
jlf -f ts,level                     # fields, default format (no template)
```

Output can be text, CSV/TSV, Markdown, HTML, XML, SQL inserts, or reshaped JSON,
because these are templates plus optional framing. Literal `{` and `}` no longer
need escaping; `$` is the interpolation sigil, and `$$` writes a literal dollar.

Built-in output shapes are recipe bundles, not separate code paths. Each uses a
single `out` template; `$rows(...)` frames the record stream when a format needs
once-before or once-after text. Escaping is unified as value modifiers (`:csv`,
`:tsv`, `:md`, `:html`, or `:none`) or as a recipe-level `escape` default. They
are runnable as recipes with `@name` (`@csv`, `@tsv`, `@md`; `@table`, `@html`,
`@json` are future) and can be inspected and copied to build a custom one.

Tabular shapes need a selected column list for `$cols(...)`. It comes from, in
order:

1. a comma-list arg or `-f` / `--fields` — `jlf @md ts,level,message`;
2. leftover bare words after a recipe or summary.

If no columns are selected, `$cols(...)` has no entries. Variable-schema input
should pass a list when the output shape depends on specific columns.

Cell widths are not computed: CSV/Markdown cells are written ragged — valid, and
renderers/editors align them; the padded example below is only for reading. The
one exception is a pretty terminal `@table` with aligned columns, which must
buffer rows (or sample the first N) to size them — the only non-streaming output.

`jlf @md ts,level,message`:

```
| ts                       | level | message    |
| ------------------------ | ----- | ---------- |
| 2024-02-06T23:52:48.349Z | INFO  | request ok |
| 2024-02-06T23:52:49.001Z | ERROR | timeout    |
```

A user-defined output format lives in `.jlf.toml` as a single `out` template,
with optional `escape`. Dynamic columns use `$cols(...)`; inside the repetition
`$key` is the column name and `$value` is the cell value. `$rows(...)` repeats
over the record stream and provides once-before and once-after framing.

```toml
[recipe.htable]
out = "<table>\n$rows( <tr>$cols( <td>${value:html}</td> )*</tr> )*</table>"
```

Invoke it with `jlf @htable ts,level,message` (`--format htable` also works).
The built-in formats use the same mechanism:

```toml
[recipe.csv]
out = "$cols( $key ),*\n$rows( $cols( ${value:csv} ),* )*"

[recipe.tsv]
out = "$cols( $key )\t*\n$rows( $cols( ${value:tsv} )\t* )*"

[recipe.md]
out = "| $cols( $key )\" | \"* |\n| $cols( --- )\" | \"* |\n$rows( | $cols( ${value:md} )\" | \"* | )*"
```

For a non-tabular layout, write a per-record `out` template. For a custom page or
report, wrap the repeated record template in `$rows(...)`. Per-format edges: a
missing field is an empty cell, Markdown escaping handles `|` and newlines, and summaries
render through the same shapes (`jlf stats latency_ms by endpoint @md`).

### The template language

A template is plain text with `$` interpolation. These pieces are the whole
language; everything else (filters, summaries, output formats) sits around them.

| Piece | Meaning |
| ----- | ------- |
| `$field`, `${a.b.c}` | a field, nested by `.` |
| `${a|b|c}` | first present wins (fallback) |
| `${field:mod}` | a modifier — styling (`:dimmed`, `:red`) or escaping (`:csv`, `:html`) |
| `${.}` | the whole record |
| `${..}` | the rest: fields not already used |
| `${ ?field }` | optional field; if empty, one adjacent space collapses |
| `${ @name }` | include a recipe body |
| `$if(f => …)$else(…)` | branch on truthiness |
| `$if(f OP literal => …)` | branch on a comparison (`==` `!=` `>` `>=` `<` `<=`) |
| `$match(f $when(pat => …) $else(…))` | dispatch on a value; the first matching arm renders |
| `$has(f => …)$else($has(g => …))` | branch on field presence |
| `$config(flag => …)$else(…)` | branch on a config/CLI flag such as `compact` |
| `$( body )"join"*` | repeat over the current record's top-level fields |
| `$path( body )"join"*` | repeat over an object or array at `path` |
| `$cols( body )"join"*` | repeat over CLI-selected columns |
| `$rows( body )"join"*` | repeat over the record stream; text before/after it is written once |

`\n` and `\t` are string escapes. Empty optional fields collapse one adjacent
space, so common log layouts do not need per-field conditional blocks:

```sh
jlf '${ ?timestamp:dimmed } ${@level} ${ ?message } ${ ?..:json }'
```

`$match( … )` is value dispatch inside a template. It is part of the `$name( … )`
self-closing block family, like `$path( … )`, `$cols( … )`, and `$if( … )`,
so it needs no end directive. Arms use `$when( PATTERN => BODY )`, splitting
on the first top-level `=>` outside quotes, with `$else( BODY )` as the default
for present values. Patterns can use literal equality, comparisons, numeric
ranges, or alternation; a missing subject matches no arm and renders nothing.

Conditionals use the same self-closing family. `$if(COND => …)` is a truthy test:
empty strings, empty arrays/objects, `null`, missing fields, and `0` are false.
It is a comparison when `COND` contains `==`, `!=`, `>`, `>=`, `<`, or `<=`.
Comparisons are numeric when both sides parse as numbers, otherwise text; quote
string literals. A missing or non-scalar field never matches a comparison.
`$has(FIELD => …)` tests field existence, including present-but-falsey values.
`$config(FLAG => …)` branches on `compact`, `no_color`, or `strict`. Branches
chain by adjacency with `$elif(… => …)` and `$else(…)`; nest `$has` inside
`$else(…)` when a later branch needs an existence test. One space after `=>` is
dropped as syntax, an all-whitespace body is kept as intentional output, and
decorative line breaks and indentation at the edges of a real body are dropped.

Recipes store a layout once and let you override one piece; the default output is
`@output`:

```toml
[recipe.output]
out = "${@timestamp}${@level}${@message}$config(compact =>  )$else(\n)${@data}"
```

To recolor levels, redefine the relevant recipe (or pass `-v level=…`); nothing
else changes. Optional collapse covers the common case, and `$has(f => …)`
remains for blocks that need exact control.

### Filter — `key=value`

```sh
jlf level=error                 # exact match
jlf level=error status=500      # AND across tokens
jlf level=error,warn            # OR within a field
jlf latency_ms>500              # range
jlf message~timeout             # contains
```

This is the model people already use in search boxes and `kubectl --selector`.

Comparisons are typed by the operator: `>` `<` `>=` `<=` parse the field as a
number (non-numeric values do not match); `=` `!=` compare as strings; `~` `!~`
test literal substring, not regex. String comparison is case-sensitive by default.
Values with spaces or shell metacharacters are quoted at the shell as usual
(`jlf 'msg~connection reset'`).

### Summarize — English words and sensible defaults

```sh
jlf count level                 # breakdown by level
jlf stats latency_ms            # min/max/mean/p50/p90/p99/count
jlf stats latency_ms by endpoint
jlf top user_id                 # most frequent values
jlf uniq session_id             # distinct count
```

No aggregate functions or parentheses. `stats <field>` returns a standard summary
rather than asking which statistics to compute.

Each verb is a single streaming pass: read a line, parse it, pull one or two
fields, update an accumulator, print at end of input. Lines where the field is
missing or (for numeric stats) non-numeric are skipped and counted, and the count
is reported so the numbers are interpretable. Field paths work like templates
(`jlf stats user.latency_ms`, `jlf top meta.region`), and filters compose after the
verb (`jlf stats latency_ms by endpoint level=error`).

**`stats <field>`** — extract the field, parse it as a number, and keep running
`count`, `min`, `max`, `sum` (for mean), plus a percentile estimate.
`min/max/mean/count` are O(1); percentiles are the only memory cost — exact
requires keeping all values (O(n)), so large inputs use a bounded-memory estimator
(t-digest or fixed-bucket histogram).

```
field: latency_ms          (12,438 values, 12 skipped: missing/non-numeric)

count    12,438
min        0.21
max     1984.50
mean      47.83
p50       12.40
p90      118.70
p99      642.10
```

**`stats <field> by <group>`** — the same accumulator kept per distinct value of
the group field, printed one row per group (default sort: `count` descending).
Memory is the number of distinct groups times a small fixed accumulator.

```
endpoint        count     min    mean     p50     p90      p99     max
/api/search     3,889    1.10   81.40   33.50  210.40   720.10  1984.5
/api/login      4,201    0.30   22.10   10.20   51.00   180.30   903.1
/api/checkout   2,560    0.50   61.20   25.10  160.20   540.00  1450.0
/api/health     1,788    0.10    1.90    1.40    3.20     8.10    40.2
```

**`top <field> [N]`** — count occurrences per distinct value, then print the top N
(default ~10) with each value's share of the total. Exact memory is O(distinct
values); very high cardinality can fall back to an approximate top-k (space-saving
/ count-min sketch).

```
user_id     count      %
u-1042      1,204    9.7%
u-8831        980    7.9%
u-2207        642    5.2%

top 3 of 3,418 distinct (12,438 values)
```

**`uniq <field>`** — track distinct values and report the count. Exact memory is
O(distinct values); a `--approx` mode can use HyperLogLog for near-constant memory
at the cost of a small error.

```
session_id: 8,217 distinct (of 12,438 values, 0 missing)
```

**`count [field]`** — with a field, a frequency breakdown of its values (like
`top` without a cutoff); with no field, a count of matching lines. Like `top` and
`uniq`, a high-cardinality field grows memory, with the same approximate fallback.

### Streaming and live updates

Summaries process input in a single pass and keep a running result, so the result
can be shown as it forms; only the _final_ value needs the end of input. Behavior
adapts to where output goes:

- **Interactive (stdout is a terminal):** the summary renders live, refreshing on a
  short interval (a sane default, no flag) and updating in place as input is
  processed, then settling on the final result at end of input. This covers a large
  file (watch the numbers converge as it reads) and a live stream (`tail -f`,
  `kubectl logs -f`), which keeps updating. The in-place redraw is a small,
  dependency-light part of the core CLI; the full interactive browser is the
  separate `jlf tui` extension.
- **Piped or redirected (not a terminal):** in-place redraw would corrupt the
  output, so the cumulative summary prints once at end of input — clean for
  `> out.csv` or `| next`.

`--window <dur>` is a separate concern from the refresh cadence above: it changes
_what is measured_, not how often the display updates. By default a summary is
cumulative — one result over all input seen. `--window 10s` instead reports
per-interval (tumbling-window) results: each interval is summarized and reset,
producing a time series. It is needed when the question is about rates or trends
rather than a running total — requests per minute, errors per 10s, or catching a
latency spike that a cumulative p99 would average away. It is also what lets a
piped, unbounded stream produce continuous output, since each window is one
complete row: `kubectl logs -f | jlf count status --window 1m`.

Memory is bounded or controllable, not unbounded:

- `count` and `stats` use fixed state: a few counters per group plus a fixed-size
  percentile sketch (t-digest), so percentiles do not grow with input length.
- `top` and `uniq` are exact by default, which costs state proportional to the
  number of _distinct_ values — a property of the question, not a leak. For a file
  it is bounded by the field's cardinality and is usually small. For
  high-cardinality fields or unbounded streams, approximate modes cap memory at a
  fixed size with small, controlled error (HyperLogLog for `uniq`, space-saving for
  `top`).

### Redact — by field

```sh
jlf --redact password,token,authorization,'*.email'   # glob on field paths
${user.email:redact}                                   # inline, same :modifier slot
```

This targets fields by path, which `sed`/`grep` cannot do reliably.

### Input — auto-detect and named formats (`--as`)

```sh
jlf -i app.log                  # detect JSON / NDJSON / logfmt / plain
jlf --as nginx -i access.log    # named format rather than an authored regex
```

JSON/NDJSON/logfmt are auto-detected. For common text log formats, `--as <name>`
selects a named parser that maps each line into fields, after which templates,
filters, summaries, and redaction work the same as for JSON. `--as` also overrides
auto-detection.

The nginx format turns this line:

```
192.168.1.10 - - [28/Jun/2026:01:12:33 +0000] "GET /api/search?q=foo HTTP/1.1" 200 1534 "-" "curl/8.4.0"
```

into fields:

```
ip=192.168.1.10  ts=2026-06-28T01:12:33Z  method=GET  path=/api/search
query=q=foo  status=200  bytes=1534  referer=-  user_agent=curl/8.4.0
```

so the normal surface applies:

```sh
jlf --as nginx '$status $method $path'
jlf count path --as nginx status>=500
jlf stats bytes by path --as nginx
```

Beyond extracting fields, a format normalizes the timestamp into a standard field
(so `by minute` / `--window` work) and uses consistent field names across formats
where it makes sense (`ts`, `status`, `ip`, `method`, `path`, `bytes`), so
`jlf status>=500` works whether the input is nginx or a JSON API log.

- Built-in formats: a curated set (nginx, apache, syslog RFC3164/5424, Rails, Go
  stdlib, klog/glog, journald, logfmt).
- User-defined formats live in `.jlf.toml`, so a team defines one once and refers
  to it by name; this keeps regex an escape hatch rather than the front door:
  ```toml
  [as.myapp]
  pattern = '^(?P<ts>\S+) (?P<level>\w+) (?P<msg>.*)$'
  ```
  ```sh
  jlf --as myapp level=error '$ts $msg'
  ```
- Formats must be inspectable (e.g. `jlf --as nginx --list-fields` lists the fields
  a format produces) so users can discover field names.
- Lines that do not match are passed through unchanged (as today) and counted.
  Multi-line records (stack traces) need line-joining behavior in the format and are
  the harder case.

### Recipes

A recipe is a saved bundle of arguments — filters, a template, and options —
invoked by name. It subsumes what were once separate "presets" and output
"formats": a recipe can be a saved command, an inline variable, or an output
format. Recipes live in `.jlf.toml` (an extension of the existing config and
variables system), so they can be committed and shared within a team:

```sh
jlf @errors
jlf -p latency-report
```

A complex invocation is written once (by hand or via the builder), named, and
reused. The keys mirror the explicit flag forms (`filter`, `out`, the summary
verbs, `format`):

```toml
# .jlf.toml
[recipe.errors]
filter = "level=error,fatal"
out = "$ts $level $message"

[recipe.latency-report]
filter = "status>=500"
stats = "latency_ms"
by = "endpoint"
format = "md"
```

`jlf @errors` runs the first; `jlf -p latency-report` runs the second.

A recipe is a set of defaults, not a fixed command — explicit arguments layer on
top:

```sh
jlf @errors status=500          # add a filter (AND with the recipe's filters)
jlf @errors level=warn          # override a same-field filter
jlf @errors '$ts $msg'        # override the recipe's template
```

The precedence matches config-to-CLI layering: a same-field filter overrides, a
different-field filter is added, and an explicit template or flag overrides the
recipe's.

## 5. Architecture

```
jlf-core   (lib)   Parser + format/filter/summarize engine. No heavy deps.
                   Reused by the CLI and any extension.

jlf        (bin)   Thin core CLI: format, key=value filter, count/stats/top,
                   redact, export. Depends only on jlf-core.

  └─ git-style dispatch: an unknown subcommand `jlf X` runs `jlf-X` from PATH
     (inheriting stdin/stdout so pipes work); if absent, it prints an install hint.

jlf-tui    (bin)   Optional extension: interactive browser/builder (ratatui).
jlf-serve  (bin)   Optional extension: local HTTP server + thin web UI.
```

The CLI is the product. Format packs (`--as nginx/syslog/rails`) are the cheapest
add. `jlf-tui` is the only extension that adds a workflow the pipe can't — live
filter, facets, follow — but it overlaps `lnav`, so it's optional and deferred;
`jlf-serve` is a later local viewer. For repeated indexed analytics over a large
file, hand off to DuckDB (`duckdb -c "select … from 'logs.ndjson'"`) rather than
build a worse store — jlf stays streaming. No agents, retention, dashboards, or
alerting; those break the no-backend promise.

### Plugin model (git / cargo / gh style)

- The core handles the lightweight cases (format/filter/summarize/redact/export)
  with no heavy dependencies.
- Heavier features (`tui`, `serve`) are separate binaries found on `PATH`.
- The contract is argv + stdin + exit code: stable and language-agnostic, so
  plugins can be written in any language.
- Discovery: the core ships a curated list of official extensions (`jlf help`
  shows them with install hints even when not installed) and suggests a package
  on an unknown subcommand.

This keeps the core small and lets users install only what they need.

### Where processing runs

For large or live data, the engine should run where the data is, and the UI
should send queries rather than receive raw data. `jlf serve` wraps the engine in
a local HTTP server; a web UI sends a query spec (filters / template /
aggregation), the engine processes near the source, and only results are streamed
back (SSE/WebSocket). This is the same split as Kibana over Elasticsearch, kept
local and without a setup step. It is the engine running near the data — there is
no client-side/browser build, since formatting JSON in a browser is already JS's
job.

## 6. What it does

- Reads structured input: JSON, NDJSON, logfmt, common formats; passes plain text
  through unchanged.
- Formats for reading: pretty, colored, level-aware (the current behavior).
- Finds: `key=value` filters, ranges, contains; live input via an upstream pipe
  (`tail -f | jlf`).
- Reshapes and exports: CSV/TSV/table/Markdown/HTML/JSON via templates.
- Redacts secrets/PII by field, safe for sharing.
- Summarizes: counts, breakdowns, top-N, numeric summaries; on a local file or a
  live pipe, without a backend; updates live in place on a terminal and prints once
  when piped.
- Browse (extension): TUI with live filter, facets, expand/collapse, follow,
  save-as-recipe.
- Serve (extension): local web UI; queries processed near the data.
- Embed: `jlf-core` crate.

## 7. How it replaces the common tools

The aim is not more power than each tool, but one structure-aware surface instead
of several syntaxes.

| Tool            | Used for                | Today                                             | jlf                                       |
| --------------- | ----------------------- | ------------------------------------------------- | ----------------------------------------- |
| grep            | find lines              | `grep error app.log` (also matches `"no errors"`) | `jlf level=error` / `jlf message~timeout` |
| cut             | extract fields          | `cut -d, -f1,3`                                   | `jlf '${ts},${level}'` (by name)            |
| awk             | field logic + condition | `awk -F, '$3>500{print $1}'`                      | `jlf latency>500 '$ts'`                  |
| jq              | JSON query/transform    | `jq 'select(.level=="error")\|{ts,msg}'`          | `jlf level=error '$ts $msg'`            |
| sed             | substitute/redact       | `sed 's/token=[^ ]*/token=***/'`                  | `jlf --redact token` (by field)           |
| awk (aggregate) | sums/counts             | `awk '{s+=$1}END{print s}'`                       | `jlf stats latency` / `jlf count level`   |

What it does not replace:

- The full programmability of awk/sed/jq, which are close to languages. jlf covers
  their common uses, not the general case.
- The storage role of Splunk/Datadog/Loki: indexed retention, cross-host
  aggregation over large volumes, alerting, dashboards, team access. jlf is not a
  datastore. It addresses the cases that do not need the platform, and can act as
  pre-processing (redact/reshape before shipping) or a last-mile viewer.
- The breadth of Vector / Fluent Bit: durable buffering, backpressure, delivery
  guarantees, many integrations. jlf can cover simple transforms
  (parse/filter/redact/reshape/forward) without their configuration languages, but
  not their delivery and routing guarantees.

## 8. Use cases

### Log query

- Trace one request across interleaved pods, with readable output:
  `kubectl logs -f deploy/api | jlf request_id=r-83f`. (`grep` produces
  false positives; `jq` outputs raw JSON.)
- Error-only follow: `tail -f app.log | jlf level=error,fatal`.
- Latency outliers: `jlf latency_ms>500 '$ts $endpoint $latency_ms'`.

### Templating (non-log)

- NDJSON to CSV: `cat events.ndjson | jlf '${user.id},$event,$ts' > events.csv`.
- `kubectl get pods -o json | jlf '${metadata.name}\t${status.phase}'`
  (simpler than jsonpath / custom-columns).
- Reports, release notes, Markdown or HTML tables.

### Analytics

- Ad-hoc percentile without a pipeline: `jlf stats latency_ms by endpoint` over a
  large file. (Otherwise: a platform, hand-rolled jq+awk+datamash, or DuckDB.)
- Error-rate over time: `jlf count level by minute`.
- Streaming aggregation: `kubectl logs -f | jlf count level --window 10s`.

This is the area least well served by current CLI tools: angle-grinder has its own
DSL and limited JSON support; Miller is capable but not tail/log-oriented; DuckDB
is SQL and batch-oriented; the platforms are server-side.

### Redaction

- `jlf --redact password,token,'*.email'` before sharing logs.

### Browse (TUI)

- `jlf tui -i app.log` or `kubectl logs -f | jlf tui`: scroll, live filter, expand
  nested JSON, follow, facet sidebar, save-as-recipe.

## 9. Roadmap and action steps

Phases are ordered so each ships standalone value and de-risks the next. Every
feature is checked against the design principles in §3.

### Phase 0 — Foundation

Goal: a light core, plugin dispatch, and a reusable engine.

- [ ] Extract the engine into a `jlf-core` library crate (`json.rs`, `format/`,
      `expand.rs`, `colors.rs`, `config.rs`) with no heavy dependencies.
- [ ] Convert the repo to a cargo workspace: `jlf-core` (lib) + `jlf` (bin),
      where `jlf` depends only on `jlf-core`.
- [ ] Implement git-style external subcommand dispatch in `jlf` (clap
      `allow_external_subcommands(true)` -> resolve `jlf-<sub>` on PATH -> exec
      with inherited stdio -> not-found message with an install hint).
- [ ] Add a curated extension list to the core for discovery (`jlf help`).
- Acceptance: `cargo build -p jlf` pulls in no axum/ratatui; `jlf foo` prints an
  install hint; existing behavior unchanged; publish `jlf-core` 0.1.

### Phase 1 — Core features

Goal: filter, summarize, redact, export, plus grammar disambiguation.

- [ ] Filters: parse positional `key <op> value` (`= != > < >= <= ~ !~`); AND
      across tokens, `a,b` OR within a value; apply as a streaming predicate.
- [ ] Lazy decode: parse numbers and unescape strings on access, preserving the
      zero-copy path while enabling numeric filters and stats.
- [ ] Summaries: `count [field]`, `stats <field>`
      (min/max/mean/p50/p90/p99/count), `top <field> [N]`, `uniq <field>`,
      `by <field>` grouping. Live in-place display when stdout is a TTY, single
      print when piped, `--window` for tumbling-window output. t-digest by default
      so `stats` stays bounded; approximate `top`/`uniq` modes for high cardinality.
- [ ] Redaction: `--redact <globs>` and inline `${x:redact}`.
- [ ] Export shapes as built-in recipe bundles (single `out` templates with
      `$rows(...)`, `$cols(...)`, and escape modifiers), run as recipes `@csv`,
      `@tsv`, `@md` (`@table`, `@html`, `@json` future); columns from a
      comma-list, `-f`/`--fields`, or leftover bare words; `:html` escape
      modifier.
- [ ] Implement and document grammar disambiguation; add examples to `jlf help`.
- Acceptance: documented examples work; golden-output tests; throughput close to
  the current core.

### Phase 2 — Recipes and the first extension (`jlf-tui`)

Goal: a low-friction entry point and shareable recipes.

- [ ] Recipes as named bundles in `.jlf.toml`, with user / workspace / project
      scopes (reusing the existing config and workspace discovery). `jlf @name`,
      with explicit CLI args layering over the recipe's defaults.
- [ ] `jlf-tui` extension (ratatui/crossterm): browse, follow, live filter,
      expand/collapse, facets; an interactive builder that previews output, prints
      the equivalent command, and saves a recipe. (Replaces the earlier
      interactive-flag idea.)
- [ ] Distribution: `cargo install jlf-tui`; brew; an optional `jlf-full` bundle.
- Acceptance: `jlf tui` runs when installed; recipe round-trip (build -> save ->
  `jlf @name`); the builder emits a valid core command.

### Phase 3 — `jlf-serve`

Goal: a local web viewer beyond the terminal, same engine.

- [ ] `jlf-serve` extension: local HTTP server + thin web UI; queries processed
      near the data; results streamed via SSE/WebSocket; facets and simple charts.
- [ ] Performance: byte-based scanning and `memchr` in the parser, which becomes
      worthwhile here since bulk parsing dominates; benchmark before/after.
- Acceptance: `serve` filters a large file server-side with low transfer.

### Phase 4 — Wider input and integration

Goal: more input formats, embedding, and a pipeline role.

- [ ] Multi-input: auto-detect JSON/NDJSON/logfmt/CSV; named formats via `--as`
      (nginx/syslog/rails, plus user-defined in `.jlf.toml`) instead of a regex
      front door; `jlf --as <name> --list-fields` to list a format's fields.
- [ ] Pipeline/sidecar mode: sinks (stdout/file/HTTP), scoped to simple transforms.
- [ ] Document the argv/stdin contract and a `jlf-<x>` template for third-party
      plugins; grow the extension list.
- Acceptance: an nginx format; an HTTP sink; a third-party plugin runs unmodified.

### Cross-cutting

- Review each feature against §3 before merging.
- Lazy decode throughout; `memchr`/byte parser when bulk parsing matters
  (Phase 3).
- Golden-output snapshot tests for formatting, filtering, and summaries.
- Semver care on `jlf-core` (a public engine and embedding surface).

### Pre-1.0 breaking changes

Since jlf is pre-1.0, these corrections are worth making now rather than after
there are users to break.

Output and scripting correctness:

- **Diagnostics to stderr.** Strict-mode parse errors currently go to stdout
  (`lib.rs`), as would the summary "skipped N" footers and the live progress
  redraw. Everything that is not the data stream should go to stderr so `| next`
  and `> out.csv` stay clean.
- **Meaningful exit codes.** Strict-mode failure currently returns success
  (exit 0). Adopt grep-style codes: `0` matched/ok, `1` no matching lines,
  `2` error.
- **`--color=auto|always|never`.** Replace the two booleans (`--color`,
  `--no-color`) with the standard tri-state.

Input model:

- **stdin + `-i`/`--input`, no bare-word filenames, no built-in follow.** Drop the
  current "do nothing when stdin is a TTY" guard so `jlf -i app.log` works without
  a pipe. Following a live source stays the upstream tool's job via a pipe.

Grammar and templates:

- **Use `$` for templates.** A bare positional is no longer a literal format
  string; the grammar uses `$` interpolation for templates and leaves bare words
  available for commands, fields, recipes, and filters.
- **Collapse optional empty fields, simplify the default recipes.** Keep named,
  reusable fragments via recipes and include directives, but make an optional
  empty field swallow one adjacent space so the default is a few plainly named
  recipes instead of many `name`/`name_fmt` twins. `$has(f => …)` stays for exact
  control.
- **Rework `expand` / `list`** into one inspection surface (`--list-fields`, an
  `explain` that prints the resolved command/format).

Smaller:

- **Fix or drop `--take`**, which currently counts blank and error lines, not
  emitted records; prefer `| head` or a `--limit` that counts output rows.
- **Settle the config schema** now: `[config]`, `[as.*]`, `[recipe.*]`, and
  `[variables]` (recipes subsume the old format/preset tables).
- **`~` / `!~` are literal substring** (decided), not regex.

## 10. Risks and mitigations

| Risk                                      | Mitigation                                                                                         |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------- |
| Scope creep                               | The design principles and the plugin model keep the core small; heavy features are opt-in binaries |
| Discovery (users unaware of extensions)   | Curated list in the core, `jlf help`, suggestions on unknown subcommand                            |
| Distribution friction (multiple installs) | A `jlf-full` bundle, one-line `cargo install`, brew                                                |
| Correctness vs. leniency                  | Lazy decode plus an opt-in strict mode; never silently wrong on a value that is accessed           |
| Crowded space                             | Differentiate on the single small surface and the shared engine across CLI/server                  |
| Overreach vs. platforms                   | Position as a local structure-aware CLI, a pre-processor, and a viewer, not a datastore            |

## 11. Success measures

- Time to a useful result for a new user without reading docs.
- Share of common tasks doable without opening `--help`.
- Core binary size and cold-start staying small.
- Adoption: installs, third-party `jlf-*` plugins.
- Recipes committed per repository (a sign of team use).

## 12. Open questions

- Grammar for `by` and `--window` time bucketing (keep it plain, avoid cron-like
  syntax).
- Percentile method (t-digest vs. exact for bounded inputs) and memory budget.
- Whole-document output: whether `$rows(...)` framing is enough or a dedicated
  `--report` mode is still useful.
- Recipe precedence and merging across user/workspace/project scopes, and how CLI
  args override or clear a recipe's filters (e.g. `level=` to clear).
- Column selection errors: how to surface `$cols(...)` formats run without an
  explicit column list.
- General row sorting beyond the `top` / `stats by` defaults (a `sort` verb or
  flag) — deferred.
- Case-insensitive filter matching (a flag or operator variant).
- Collapse-on-empty: exactly which adjacent whitespace an empty field eats (one
  space, leading vs trailing), and how to opt out where a literal gap is wanted.
- Minimum query-spec wire format for `serve`.
