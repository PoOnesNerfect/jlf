use std::io::{self, BufRead, IsTerminal, Write};

use clap::{Parser, Subcommand};
use jlf_core::{expanded_format, get_config, ConfigFile, Formatter, Json};
use owo_colors::OwoColorize;

use clap::ValueEnum;

#[derive(Copy, Clone, Debug, PartialEq, ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Format template, filters (key=value), and fields. [default: {&output}]
    args: Vec<String>,

    #[command(flatten)]
    variables: Variables,

    /// Color output: auto (default), always, or never.
    #[arg(long = "color", value_enum, default_value_t = ColorWhen::Auto)]
    color: ColorWhen,

    /// Disable color output (shortcut for --color=never).
    #[arg(short = 'n', long = "no-color", default_value_t = false)]
    no_color: bool,

    /// Display log in a compact format.
    #[arg(short = 'c', long = "compact", default_value_t = false)]
    compact: bool,

    /// If log line is not valid JSON, then report it and exit, instead of
    /// printing the line as is.
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,

    /// Take only the first N emitted records.
    #[arg(short = 't', long = "take")]
    take: Option<usize>,

    /// Input file(s); repeatable. Defaults to stdin.
    #[arg(short = 'i', long = "input", value_name = "FILE")]
    input: Vec<String>,

    /// Fields/columns to show (comma-separated), e.g. -f ts,level,msg.
    #[arg(short = 'f', long = "fields", value_name = "FIELDS", value_delimiter = ',')]
    fields: Vec<String>,

    /// Redact fields by name; comma-separated globs (e.g. password,token,*.email).
    #[arg(short = 'r', long = "redact", value_name = "FIELDS", value_delimiter = ',')]
    redact: Vec<String>,

    /// Output as CSV (columns from a comma-list arg, e.g. `jlf --csv ts,level,msg`).
    #[arg(long)]
    csv: bool,
    /// Output as TSV.
    #[arg(long)]
    tsv: bool,
    /// Output as a Markdown table.
    #[arg(long)]
    md: bool,

    /// Render with a named output format: a built-in (`csv`/`tsv`/`md`) or a
    /// custom `[format.NAME]` from your config.
    #[arg(long = "format", value_name = "NAME")]
    format_name: Option<String>,

    /// Use a saved preset from `[preset.NAME]` (also: `@NAME` as a bare arg).
    #[arg(short = 'p', long = "preset", value_name = "NAME")]
    preset: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print variable with its inner variables expanded.
    /// If no variable is specified, the default format string will be used.
    Expand {
        /// Variable to expand
        variable: Option<String>,

        #[command(flatten)]
        variables: Variables,
    },
    /// List all variables
    List {
        #[command(flatten)]
        variables: Variables,
    },
    /// Count lines, or a frequency breakdown of a field's values.
    Count {
        #[command(flatten)]
        out: OutFmt,
        /// Optional field to break down by; bare args without `{}` or operators.
        /// Tokens with operators are filters.
        args: Vec<String>,
    },
    /// Numeric summary of a field: count/min/max/mean/p50/p90/p99.
    Stats {
        #[command(flatten)]
        out: OutFmt,
        /// The numeric field, optional `by <group>`, and key=value filters.
        args: Vec<String>,
    },
    /// Most frequent values of a field (top N, default 10).
    Top {
        #[command(flatten)]
        out: OutFmt,
        /// Field, optional N, and key=value filters.
        args: Vec<String>,
    },
    /// Number of distinct values of a field.
    Uniq {
        #[command(flatten)]
        out: OutFmt,
        /// Field and key=value filters.
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, Default, clap::Args)]
struct OutFmt {
    /// Output summary as CSV.
    #[arg(long)]
    csv: bool,
    /// Output summary as TSV.
    #[arg(long)]
    tsv: bool,
    /// Output summary as a Markdown table.
    #[arg(long)]
    md: bool,
}

impl OutFmt {
    fn mode(&self) -> Option<Table> {
        let name = if self.csv {
            "csv"
        } else if self.tsv {
            "tsv"
        } else if self.md {
            "md"
        } else {
            return None;
        };
        Table::builtin(name)
    }
}

#[derive(Debug, clap::Args)]
struct Variables {
    /// Pass variable as KEY=VALUE format; can be passed multiple times.
    #[arg(short = 'v', long = "variable", value_name = "KEY=VALUE")]
    variables: Option<Vec<String>>,
}

pub fn run() -> Result<(), color_eyre::Report> {
    color_eyre::install()?;

    let Args {
        args,
        variables,
        color,
        no_color,
        compact,
        strict,
        take,
        input,
        fields: fields_flag,
        redact,
        csv,
        tsv,
        md,
        format_name,
        preset,
        command,
    } = Args::parse();

    // Classify bare args: `@name` is a preset, a token with `{` the template, one
    // with an operator a filter, a comma-list the field/column list.
    let mut format = None;
    let mut filter_strs: Vec<String> = Vec::new();
    let mut fields: Vec<String> = fields_flag;
    let mut preset_name = preset;
    for a in args {
        if let Some(name) = a.strip_prefix('@').filter(|n| !n.is_empty()) {
            preset_name.get_or_insert_with(|| name.to_owned());
        } else if a.contains('{') {
            format = Some(a);
        } else if jlf_core::Filter::parse(&a).is_some() {
            filter_strs.push(a);
        } else if a.contains(',') || (csv || tsv || md) {
            fields = a.split(',').map(str::to_owned).collect();
        }
    }

    let mut cfg = get_config()?;

    // Build the set of active condition flags for recipe `[recipe.NAME.<cond>]`
    // overrides: a flag is active if set on the CLI, in `[config]`, or by the
    // preset being invoked (peeked from the parsed presets before translation).
    let preset_compact = preset_name
        .as_ref()
        .and_then(|n| cfg.presets.get(n))
        .and_then(|p| p.compact)
        .unwrap_or(false);
    let mut active: Vec<&str> = Vec::new();
    if compact || preset_compact || cfg.config.compact.unwrap_or(false) {
        active.push("compact");
    }
    if no_color || color == ColorWhen::Never || cfg.config.no_color.unwrap_or(false) {
        active.push("no_color");
    }
    if strict || cfg.config.strict.unwrap_or(false) {
        active.push("strict");
    }
    cfg.resolve_recipes(&active);

    let ConfigFile {
        mut config,
        variables: config_variables,
        formats,
        presets,
        tables,
        ..
    } = cfg;

    let mut compact = compact;
    let mut redact = redact;
    let mut format_name = format_name;
    let mut preset_summary = None;

    // Resolve a preset: its values are defaults that explicit args layer over —
    // a same-field filter overrides, a different-field filter is added, and an
    // explicit template/fields/format/redact wins.
    if let Some(name) = &preset_name {
        let Some(def) = presets.get(name) else {
            eprintln!("jlf: unknown preset `{name}` (no [preset.{name}] in config)");
            std::process::exit(2);
        };
        filter_strs = merge_filter_tokens(def.filter.as_deref().unwrap_or(""), filter_strs);
        if format.is_none() && fields.is_empty() {
            if let Some(t) = &def.template {
                format = Some(t.clone());
            } else if let Some(fl) = &def.fields {
                fields = fl.split(',').map(str::to_owned).collect();
            }
        }
        if redact.is_empty() {
            if let Some(r) = &def.redact {
                redact = r.split(',').map(str::to_owned).collect();
            }
        }
        compact = compact || def.compact.unwrap_or(false);
        if format_name.is_none() && !(csv || tsv || md) {
            format_name = def.format.clone();
        }
        if command.is_none() {
            preset_summary = preset_summary_from(def);
        }
    }

    let filters: Vec<jlf_core::Filter> = filter_strs
        .iter()
        .filter_map(|s| jlf_core::Filter::parse(s))
        .collect();

    // A preset can run a summary directly (`stats = "latency_ms"`, etc.).
    if let Some((verb, field, by, n)) = preset_summary {
        let mode = resolve_table(format_name.as_deref(), csv, tsv, md, &tables);
        let mut sargs = Vec::new();
        if !field.is_empty() {
            sargs.push(field);
        }
        if let Some(by) = by {
            sargs.push("by".to_owned());
            sargs.push(by);
        }
        if verb == "top" && n > 0 {
            sargs.push(n.to_string());
        }
        sargs.extend(filter_strs.iter().cloned());
        return match verb.as_str() {
            "count" => run_count(sargs, mode, &input),
            "stats" => run_stats(sargs, mode, &input),
            "top" => run_top(sargs, mode, &input),
            _ => run_uniq(sargs, &input),
        };
    }

    // `--csv/--tsv/--md`, `--format NAME`, or a preset's `format` that names a
    // column table (built-in or a recipe with a `separator`) export directly.
    if let Some(table) = resolve_table(format_name.as_deref(), csv, tsv, md, &tables) {
        return run_export(fields, filters, table, &input);
    }

    // A bare -f/--fields with no template builds a simple "{a} {b}" template.
    if format.is_none() && !fields.is_empty() {
        format = Some(fields.iter().map(|f| format!("{{{f}}}")).collect::<Vec<_>>().join(" "));
    }

    // A non-built-in `--format NAME` selects a custom `[format.NAME]`.
    if let Some(name) = format_name {
        let Some(def) = formats.get(&name) else {
            eprintln!("jlf: unknown format `{name}` (not a built-in and no [format.{name}] in config)");
            std::process::exit(2);
        };
        return run_custom_format(def, filters, redact, &input, &config_variables);
    }

    if let Some(format) = format {
        config.format = Some(format);
    }
    if compact {
        config.compact = Some(true);
    }
    if no_color {
        config.no_color = Some(true);
    }
    if strict {
        config.strict = Some(true);
    }
    let format = config.format.unwrap_or_else(|| "{&output}".to_owned());
    let compact = config.compact.unwrap_or(false);
    let no_color = config.no_color.unwrap_or(false);
    let strict = config.strict.unwrap_or(false);

    if let Some(command) = command {
        match command {
            Command::Expand {
                variable,
                variables: Variables { variables },
            } => {
                let variables = get_variables(config_variables, variables);
                let format = variable.map(|e| format!("{{&{e}}}")).unwrap_or(format);

                println!("{}", expanded_format(&format, &variables));
            }
            Command::List { variables } => {
                let variables = get_variables(config_variables, variables.variables);
                let width = variables.iter().map(|(k, _)| k.len()).max().unwrap();
                for (k, v) in variables {
                    println!("{:width$} = {v}", k.bold(), width = width);
                }
            }
            // Preset filters (if any) apply to an explicit summary subcommand too.
            Command::Count { args, out } => {
                return run_count(prepend(&filter_strs, args), out.mode(), &input)
            }
            Command::Stats { args, out } => {
                return run_stats(prepend(&filter_strs, args), out.mode(), &input)
            }
            Command::Top { args, out } => {
                return run_top(prepend(&filter_strs, args), out.mode(), &input)
            }
            Command::Uniq { args, out: _ } => {
                return run_uniq(prepend(&filter_strs, args), &input)
            }
        }

        return Ok(());
    }

    let stdout = io::stdout();
    let no_color = match color {
        ColorWhen::Always => false,
        ColorWhen::Never => true,
        ColorWhen::Auto => no_color || !stdout.is_terminal(),
    };

    // Buffer stdout: the formatter emits many small writes per record, and a
    // bare StdoutLock is line-buffered (a flush per '\n'). A BufWriter
    // collapses those into a few large writes.
    let mut stdout = io::BufWriter::with_capacity(64 * 1024, stdout.lock());

    let variables = get_variables(config_variables, variables.variables);
    let expanded = expanded_format(&format, &variables);
    let formatter = Formatter::new(&expanded, no_color, compact)?;

    let mut buf = open_input(&input)?;

    // input line read from stdin (allocation reused across iterations)
    let mut line = String::new();

    // formatted output for one record (allocation reused across iterations)
    let mut out = String::new();

    // how many records have we emitted?
    let mut taken = 0;

    while buf.read_line(&mut line)? != 0 {
        // Only run the (allocating) ANSI strip when the line actually
        // contains an escape byte. JSON logs almost never do, so this skips
        // a per-line allocation + full-line scan on the common path.
        let stripped;
        let input: &str = if line.as_bytes().contains(&0x1b) {
            stripped = strip_ansi_escapes::strip_str(&line);
            &stripped
        } else {
            &line
        };

        if !input.trim().is_empty() {
            // `json` is scoped to this iteration so its borrows of `input`
            // end before the next read; this is what lets us avoid the
            // previous lifetime-laundering `unsafe` block.
            let mut json = Json::Null;
            match json.parse_replace(input) {
                Ok(()) => {
                    if !filters.is_empty() && !jlf_core::matches_all(&filters, &json) {
                        line.clear();
                        continue;
                    }
                    if !redact.is_empty() {
                        jlf_core::redact(&mut json, &redact);
                    }
                    out.clear();
                    formatter.as_log(&json).write_fmt(&mut out)?;
                    out.push('\n');
                    stdout.write_all(out.as_bytes())?;
                }
                Err(e) => {
                    if strict {
                        if no_color {
                            eprintln!("{:?}", e);
                        } else {
                            eprintln!("{:?}", e.red());
                        }
                        stdout.flush()?;
                        std::process::exit(1);
                    }

                    // not strict: echo the line unchanged (already includes
                    // its trailing newline from read_line)
                    if no_color {
                        stdout.write_all(input.as_bytes())?;
                    } else {
                        stdout.write_all(line.as_bytes())?;
                    }
                }
            }

            // take only N emitted records if specified
            if let Some(take) = take.as_ref() {
                taken += 1;
                if taken >= *take {
                    line.clear();
                    break;
                }
            }
        }

        line.clear();
    }

    stdout.flush()?;

    Ok(())
}

/// Prepend preset-derived filter tokens to a summary subcommand's args.
fn prepend(filter_strs: &[String], mut args: Vec<String>) -> Vec<String> {
    let mut out = filter_strs.to_vec();
    out.append(&mut args);
    out
}

/// Merge a preset's `where` filters under explicit ones: an explicit filter on a
/// field drops the preset's filters on that same field; others are kept.
fn merge_filter_tokens(preset_where: &str, explicit: Vec<String>) -> Vec<String> {
    let explicit_keys: std::collections::HashSet<String> = explicit
        .iter()
        .filter_map(|s| jlf_core::Filter::parse(s).map(|f| f.key()))
        .collect();
    let mut merged: Vec<String> = preset_where
        .split_whitespace()
        .filter(|t| {
            jlf_core::Filter::parse(t)
                .map(|f| !explicit_keys.contains(&f.key()))
                .unwrap_or(false)
        })
        .map(str::to_owned)
        .collect();
    merged.extend(explicit);
    merged
}

/// Extract a summary verb from a preset, if it defines one.
/// Returns `(verb, field, by, n)`.
fn preset_summary_from(
    def: &jlf_core::PresetDef,
) -> Option<(String, String, Option<String>, usize)> {
    if let Some(f) = &def.count {
        Some(("count".into(), f.clone(), def.by.clone(), def.n.unwrap_or(10)))
    } else if let Some(f) = &def.stats {
        Some(("stats".into(), f.clone(), def.by.clone(), 0))
    } else if let Some(f) = &def.top {
        Some(("top".into(), f.clone(), def.by.clone(), def.n.unwrap_or(10)))
    } else {
        def.uniq
            .as_ref()
            .map(|f| ("uniq".into(), f.clone(), None, 0))
    }
}

/// The output-format name selected by `--csv/--tsv/--md` or `--format NAME`.
/// `None` means the default (template) output.
fn table_name(format_name: Option<&str>, csv: bool, tsv: bool, md: bool) -> Option<String> {
    if md {
        Some("md".into())
    } else if tsv {
        Some("tsv".into())
    } else if csv {
        Some("csv".into())
    } else {
        format_name.map(str::to_owned)
    }
}

/// Resolve the selected output format to a column [`Table`], if it names one
/// (built-in `csv`/`tsv`/`md` or a user recipe with a `separator`). A framed
/// format name (e.g. `report`) resolves to `None` here and is handled elsewhere.
fn resolve_table(
    format_name: Option<&str>,
    csv: bool,
    tsv: bool,
    md: bool,
    tables: &std::collections::HashMap<String, jlf_core::TableDef>,
) -> Option<Table> {
    let name = table_name(format_name, csv, tsv, md)?;
    tables.get(&name).map(Table::from_def)
}

/// Open the input: stdin when no files are given, otherwise the files chained in
/// order as one stream.
fn open_input(files: &[String]) -> io::Result<Box<dyn BufRead>> {
    if files.is_empty() {
        Ok(Box::new(io::BufReader::new(io::stdin())))
    } else {
        let mut readers = Vec::with_capacity(files.len());
        for f in files {
            readers.push(std::fs::File::open(f)?);
        }
        Ok(Box::new(io::BufReader::new(MultiReader { readers, pos: 0 })))
    }
}

/// Reads a list of files back-to-back as a single contiguous stream.
struct MultiReader {
    readers: Vec<std::fs::File>,
    pos: usize,
}

impl io::Read for MultiReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pos < self.readers.len() {
            let n = self.readers[self.pos].read(buf)?;
            if n != 0 {
                return Ok(n);
            }
            self.pos += 1;
        }
        Ok(0)
    }
}

fn get_variables(
    from_config: Option<Vec<(String, String)>>,
    args: Option<Vec<String>>,
) -> Vec<(String, String)> {
    let mut variables = jlf_core::default_variables();

    if let Some(from_config) = from_config {
        for (k2, v2) in from_config {
            let v = variables
                .iter_mut()
                .find_map(|(k, v)| (k == &k2).then_some(v));

            if let Some(v) = v {
                *v = v2;
            } else {
                variables.push((k2, v2));
            }
        }
    }

    if let Some(args) = args {
        for (key, val) in args.iter().filter_map(|e| e.split_once('=')) {
            let v_mut = variables
                .iter_mut()
                .find_map(|(k, v)| (k == key).then_some(v));
            if let Some(v) = v_mut {
                *v = val.to_owned();
            } else {
                variables.push((key.to_owned(), val.to_owned()));
            }
        }
    }

    variables
}

/// `count` subcommand: count matching lines, or a field's value frequencies.
fn run_count(args: Vec<String>, fmt: Option<Table>, input: &[String]) -> Result<(), color_eyre::Report> {
    let mut field: Option<Vec<String>> = None;
    let mut filters = Vec::new();
    for a in args {
        if let Some(f) = jlf_core::Filter::parse(&a) {
            filters.push(f);
        } else {
            field = Some(a.split('.').map(str::to_owned).collect());
        }
    }

    let mut buf = open_input(input)?;
    let mut line = String::new();
    let mut total: u64 = 0;
    let mut by: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    while buf.read_line(&mut line)? != 0 {
        if !line.trim().is_empty() {
            let mut json = Json::Null;
            if json.parse_replace(&line).is_ok()
                && (filters.is_empty() || jlf_core::matches_all(&filters, &json))
            {
                total += 1;
                if let Some(path) = &field {
                    let key = scalar(resolve(&json, path)).unwrap_or("∅");
                    *by.entry(key.to_owned()).or_insert(0) += 1;
                }
            }
        }
        line.clear();
    }

    let mut stdout = io::BufWriter::new(io::stdout().lock());
    if field.is_some() {
        let mut rows: Vec<_> = by.into_iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1));
        match fmt {
            Some(m) => {
                write_table(&mut stdout, &["count", "value"], rows.iter().map(|(k, n)| vec![n.to_string(), k.clone()]), &m)?;
            }
            None => {
                writeln!(stdout, "{:>10}  value", "count")?;
                for (k, n) in &rows {
                    writeln!(stdout, "{n:>10}  {k}")?;
                }
                writeln!(stdout, "{total:>10}  total")?;
            }
        }
    } else {
        writeln!(stdout, "{total}")?;
    }
    stdout.flush()?;
    Ok(())
}

/// Render summary rows through a table with a header.
fn write_table(
    w: &mut impl Write,
    head: &[&str],
    rows: impl Iterator<Item = Vec<String>>,
    table: &Table,
) -> io::Result<()> {
    let head: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    table.write_header(w, &head)?;
    for r in rows {
        writeln!(w, "{}", table.row(r.into_iter()))?;
    }
    Ok(())
}

fn resolve<'a>(json: &'a Json<'a>, path: &[String]) -> &'a Json<'a> {
    let mut cur = json;
    for seg in path {
        cur = match seg.parse::<usize>() {
            Ok(i) => cur.get_i(i),
            Err(_) => cur.get(seg),
        };
    }
    cur
}

/// A JSON scalar (string, number, bool) rendered as a borrowed `&str`, if it is
/// one. Objects, arrays and null yield `None`.
fn scalar<'a>(json: &'a Json<'a>) -> Option<&'a str> {
    json.as_str().or_else(|| json.as_value())
}

/// `stats <field> [by <group>]`: numeric summary, optionally grouped.
fn run_stats(args: Vec<String>, fmt: Option<Table>, input: &[String]) -> Result<(), color_eyre::Report> {
    let mut field: Option<Vec<String>> = None;
    let mut group: Option<Vec<String>> = None;
    let mut filters = Vec::new();
    let mut it = args.into_iter().peekable();
    while let Some(a) = it.next() {
        if a == "by" {
            if let Some(g) = it.next() {
                group = Some(g.split('.').map(str::to_owned).collect());
            }
        } else if let Some(f) = jlf_core::Filter::parse(&a) {
            filters.push(f);
        } else if field.is_none() {
            field = Some(a.split('.').map(str::to_owned).collect());
        }
    }
    let Some(field) = field else {
        eprintln!("stats: a numeric field is required, e.g. `jlf stats latency_ms`");
        std::process::exit(2);
    };

    let mut buf = open_input(input)?;
    let mut line = String::new();
    let mut groups: std::collections::HashMap<String, jlf_core::Digest> =
        std::collections::HashMap::new();
    let mut skipped: u64 = 0;
    while buf.read_line(&mut line)? != 0 {
        if !line.trim().is_empty() {
            let mut json = Json::Null;
            if json.parse_replace(&line).is_ok()
                && (filters.is_empty() || jlf_core::matches_all(&filters, &json))
            {
                let v = resolve(&json, &field);
                match scalar(v).and_then(|s| s.parse::<f64>().ok()) {
                    Some(n) => {
                        let key = group
                            .as_ref()
                            .map(|g| scalar(resolve(&json, g)).unwrap_or("∅").to_owned())
                            .unwrap_or_default();
                        groups.entry(key).or_default().add(n);
                    }
                    None => skipped += 1,
                }
            }
        }
        line.clear();
    }

    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut rows: Vec<_> = groups.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1.count()));
    // Percentiles come from the digest: exact for normal inputs, t-digest
    // approximation past ~50k values per group (count/min/max/mean stay exact).
    let summarize = |d: &mut jlf_core::Digest| {
        (
            d.count(),
            d.min(),
            d.max(),
            d.mean(),
            d.quantile(0.5),
            d.quantile(0.9),
            d.quantile(0.99),
        )
    };
    if let Some(m) = fmt {
        let head: &[&str] = &["group", "count", "min", "mean", "p50", "p90", "p99"];
        write_table(&mut stdout, head, rows.iter_mut().map(|(k, d)| {
            let (n, min, _mx, mean, p50, p90, p99) = summarize(d);
            vec![k.clone(), n.to_string(), fmt2(min), fmt2(mean), fmt2(p50), fmt2(p90), fmt2(p99)]
        }), &m)?;
    } else {
        if group.is_some() {
            writeln!(stdout, "{:<24} {:>8} {:>10} {:>10} {:>10} {:>10}", "group", "count", "min", "mean", "p50", "p99")?;
        }
        for (k, mut d) in rows {
            let (n, min, max, mean, p50, p90, p99) = summarize(&mut d);
            if group.is_some() {
                writeln!(stdout, "{k:<24} {n:>8} {min:>10.2} {mean:>10.2} {p50:>10.2} {p99:>10.2}")?;
            } else {
                writeln!(stdout, "count {n}\nmin   {min:.2}\nmax   {max:.2}\nmean  {mean:.2}\np50   {p50:.2}\np90   {p90:.2}\np99   {p99:.2}")?;
            }
        }
    }
    if skipped > 0 {
        eprintln!("(skipped {skipped} missing/non-numeric)");
    }
    stdout.flush()?;
    Ok(())
}

fn fmt2(v: f64) -> String {
    format!("{v:.2}")
}

/// Read field-frequency counts plus total, applying filters; field path + filters
/// come from `args`, with an optional trailing integer N for `top`.
type FieldCounts = (Vec<(String, u64)>, u64, usize);

fn collect_field(args: Vec<String>, input: &[String]) -> Result<FieldCounts, color_eyre::Report> {
    let mut field: Option<Vec<String>> = None;
    let mut filters = Vec::new();
    let mut n = 10usize;
    for a in args {
        if let Some(f) = jlf_core::Filter::parse(&a) {
            filters.push(f);
        } else if let Ok(parsed) = a.parse::<usize>() {
            n = parsed;
        } else if field.is_none() {
            field = Some(a.split('.').map(str::to_owned).collect());
        }
    }
    let mut buf = open_input(input)?;
    let mut line = String::new();
    let mut total = 0u64;
    let mut by: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    while buf.read_line(&mut line)? != 0 {
        if !line.trim().is_empty() {
            let mut json = Json::Null;
            if json.parse_replace(&line).is_ok()
                && (filters.is_empty() || jlf_core::matches_all(&filters, &json))
            {
                total += 1;
                if let Some(p) = &field {
                    let v = resolve(&json, p);
                    let k = scalar(v).unwrap_or("∅");
                    *by.entry(k.to_owned()).or_insert(0) += 1;
                }
            }
        }
        line.clear();
    }
    Ok((by.into_iter().collect(), total, n))
}

fn run_top(args: Vec<String>, fmt: Option<Table>, input: &[String]) -> Result<(), color_eyre::Report> {
    let (mut rows, total, n) = collect_field(args, input)?;
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    let mut out = io::BufWriter::new(io::stdout().lock());
    if let Some(m) = fmt {
        write_table(&mut out, &["count", "share", "value"], rows.iter().take(n).map(|(k, c)| {
            let p = if total > 0 { *c as f64 / total as f64 * 100.0 } else { 0.0 };
            vec![c.to_string(), format!("{p:.1}%"), k.clone()]
        }), &m)?;
    } else {
        writeln!(out, "{:>10}  {:>6}  value", "count", "share")?;
        for (k, c) in rows.iter().take(n) {
            let p = if total > 0 { *c as f64 / total as f64 * 100.0 } else { 0.0 };
            writeln!(out, "{c:>10}  {p:>5.1}%  {k}")?;
        }
        writeln!(out, "top {} of {} distinct ({total} values)", n.min(rows.len()), rows.len())?;
    }
    out.flush()?;
    Ok(())
}

fn run_uniq(args: Vec<String>, input: &[String]) -> Result<(), color_eyre::Report> {
    let (rows, total, _) = collect_field(args, input)?;
    println!("{} distinct (of {total} values)", rows.len());
    Ok(())
}

/// A column table output format built from a [`jlf_core::TableDef`]: cells are
/// escaped per `quote`, joined by `sep`, wrapped per row, with an optional
/// Markdown-style `rule` row after the header.
#[derive(Clone)]
struct Table {
    sep: String,
    quote: Quote,
    prefix: String,
    suffix: String,
    rule: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Quote {
    Csv,
    Tsv,
    Md,
    None,
}

impl Table {
    fn from_def(def: &jlf_core::TableDef) -> Table {
        Table {
            sep: def.separator.clone(),
            quote: match def.escape.as_deref() {
                Some("csv") => Quote::Csv,
                Some("tsv") => Quote::Tsv,
                Some("md") => Quote::Md,
                _ => Quote::None,
            },
            prefix: def.row_prefix.clone().unwrap_or_default(),
            suffix: def.row_suffix.clone().unwrap_or_default(),
            rule: def.rule.clone(),
        }
    }

    /// A built-in table by name (`csv`/`tsv`/`md`), for summary output which
    /// doesn't consult user config.
    fn builtin(name: &str) -> Option<Table> {
        jlf_core::builtin_tables().get(name).map(Table::from_def)
    }

    /// Escape one cell for this table's quoting style.
    fn esc(&self, s: &str) -> String {
        match self.quote {
            Quote::Csv => {
                if s.contains('"') || s.contains('\n') || s.contains('\r') || s.contains(&self.sep) {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s.to_owned()
                }
            }
            Quote::Tsv => s.replace(['\t', '\n', '\r'], " "),
            Quote::Md => s.replace('|', "\\|").replace('\n', "<br>"),
            Quote::None => s.to_owned(),
        }
    }

    /// Render one row: prefix + escaped cells joined by sep + suffix.
    fn row(&self, cells: impl Iterator<Item = String>) -> String {
        let body = cells.map(|c| self.esc(&c)).collect::<Vec<_>>().join(&self.sep);
        format!("{}{}{}", self.prefix, body, self.suffix)
    }

    /// Write the header row (unescaped column names) and any rule row.
    fn write_header(&self, w: &mut impl Write, cols: &[String]) -> io::Result<()> {
        writeln!(w, "{}", self.row(cols.iter().cloned()))?;
        if let Some(rule) = &self.rule {
            let cells = cols.iter().map(|_| rule.clone());
            let body = cells.collect::<Vec<_>>().join(&self.sep);
            writeln!(w, "{}{}{}", self.prefix, body, self.suffix)?;
        }
        Ok(())
    }
}

/// CSV/TSV/Markdown export with per-cell escaping. Columns from `fields`.
/// Render records with a user-defined `[format.NAME]`: `header` and `footer`
/// emitted once around per-record `row` templates, with interpolated values
/// escaped per `escape`.
fn run_custom_format(
    def: &jlf_core::FormatDef,
    filters: Vec<jlf_core::Filter>,
    redact: Vec<String>,
    input: &[String],
    config_variables: &Option<Vec<(String, String)>>,
) -> Result<(), color_eyre::Report> {
    let escape = match def.escape.as_deref() {
        None => jlf_core::Escape::None,
        Some(name) => {
            let Some(e) = jlf_core::Escape::from_name(name) else {
                eprintln!("jlf: unknown escape `{name}` (expected `none` or `html`)");
                std::process::exit(2);
            };
            e
        }
    };

    let variables = get_variables(config_variables.clone(), None);
    let expanded = expanded_format(&def.row, &variables);
    let formatter = Formatter::new(&expanded, true, false)?.with_escape(escape);

    let mut buf = open_input(input)?;
    let mut stdout = io::BufWriter::with_capacity(64 * 1024, io::stdout().lock());

    if let Some(header) = &def.header {
        write!(stdout, "{header}")?;
    }

    let mut line = String::new();
    let mut out = String::new();
    while buf.read_line(&mut line)? != 0 {
        if !line.trim().is_empty() {
            let mut json = Json::Null;
            if json.parse_replace(&line).is_ok()
                && (filters.is_empty() || jlf_core::matches_all(&filters, &json))
            {
                if !redact.is_empty() {
                    jlf_core::redact(&mut json, &redact);
                }
                out.clear();
                formatter.as_log(&json).write_fmt(&mut out)?;
                out.push('\n');
                stdout.write_all(out.as_bytes())?;
            }
        }
        line.clear();
    }

    if let Some(footer) = &def.footer {
        write!(stdout, "{footer}")?;
    }
    stdout.flush()?;
    Ok(())
}

fn run_export(fields: Vec<String>, filters: Vec<jlf_core::Filter>, table: Table, input: &[String]) -> Result<(), color_eyre::Report> {
    let cols: Vec<Vec<String>> = fields.iter().map(|f| f.split('.').map(str::to_owned).collect()).collect();
    let mut stdout = io::BufWriter::with_capacity(64 * 1024, io::stdout().lock());

    table.write_header(&mut stdout, &fields)?;

    let mut buf = open_input(input)?;
    let mut line = String::new();
    while buf.read_line(&mut line)? != 0 {
        if !line.trim().is_empty() {
            let mut json = Json::Null;
            if json.parse_replace(&line).is_ok()
                && (filters.is_empty() || jlf_core::matches_all(&filters, &json))
            {
                let cells = cols
                    .iter()
                    .map(|p| scalar(resolve(&json, p)).unwrap_or("").to_owned());
                writeln!(stdout, "{}", table.row(cells))?;
            }
        }
        line.clear();
    }
    stdout.flush()?;
    Ok(())
}
