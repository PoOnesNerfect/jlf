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
    fn mode(&self) -> Option<Sep> {
        if self.csv {
            Some(Sep::Csv)
        } else if self.tsv {
            Some(Sep::Tsv)
        } else if self.md {
            Some(Sep::Md)
        } else {
            None
        }
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
        command,
    } = Args::parse();

    // Classify bare args: a token with `{` is the template, one with an operator
    // is a filter, a comma-list of plain names is the field/column list.
    let mut format = None;
    let mut filters = Vec::new();
    let mut fields: Vec<String> = fields_flag;
    for a in args {
        if a.contains('{') {
            format = Some(a);
        } else if let Some(f) = jlf_core::Filter::parse(&a) {
            filters.push(f);
        } else if a.contains(',') || (csv || tsv || md) {
            fields = a.split(',').map(str::to_owned).collect();
        }
    }

    // `--format csv|tsv|md` are shorthands for the built-in exporters.
    let builtin_export = if md || format_name.as_deref() == Some("md") {
        Some(Sep::Md)
    } else if tsv || format_name.as_deref() == Some("tsv") {
        Some(Sep::Tsv)
    } else if csv || format_name.as_deref() == Some("csv") {
        Some(Sep::Csv)
    } else {
        None
    };
    if let Some(mode) = builtin_export {
        return run_export(fields, filters, mode, &input);
    }

    // A bare -f/--fields with no template builds a simple "{a} {b}" template.
    if format.is_none() && !fields.is_empty() {
        format = Some(fields.iter().map(|f| format!("{{{f}}}")).collect::<Vec<_>>().join(" "));
    }

    let ConfigFile {
        mut config,
        variables: config_variables,
        formats,
        presets: _,
    } = get_config()?;

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
            Command::Count { args, out } => return run_count(args, out.mode(), &input),
            Command::Stats { args, out } => return run_stats(args, out.mode(), &input),
            Command::Top { args, out } => return run_top(args, out.mode(), &input),
            Command::Uniq { args, out: _ } => return run_uniq(args, &input),
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
fn run_count(args: Vec<String>, fmt: Option<Sep>, input: &[String]) -> Result<(), color_eyre::Report> {
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
                write_table(&mut stdout, &["count", "value"], rows.iter().map(|(k, n)| vec![n.to_string(), k.clone()]), m)?;
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

/// Render rows in CSV/TSV/MD with a header.
fn write_table(
    w: &mut impl Write,
    head: &[&str],
    rows: impl Iterator<Item = Vec<String>>,
    mode: Sep,
) -> io::Result<()> {
    let join = |cells: &[String]| -> String {
        match mode {
            Sep::Csv => cells.iter().map(|c| esc(c, mode)).collect::<Vec<_>>().join(","),
            Sep::Tsv => cells.iter().map(|c| esc(c, mode)).collect::<Vec<_>>().join("\t"),
            Sep::Md => format!("| {} |", cells.iter().map(|c| esc(c, mode)).collect::<Vec<_>>().join(" | ")),
        }
    };
    let head: Vec<String> = head.iter().map(|s| s.to_string()).collect();
    writeln!(w, "{}", join(&head))?;
    if matches!(mode, Sep::Md) {
        writeln!(w, "| {} |", head.iter().map(|_| "---").collect::<Vec<_>>().join(" | "))?;
    }
    for r in rows {
        writeln!(w, "{}", join(&r))?;
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
fn run_stats(args: Vec<String>, fmt: Option<Sep>, input: &[String]) -> Result<(), color_eyre::Report> {
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
        }), m)?;
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

fn run_top(args: Vec<String>, fmt: Option<Sep>, input: &[String]) -> Result<(), color_eyre::Report> {
    let (mut rows, total, n) = collect_field(args, input)?;
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    let mut out = io::BufWriter::new(io::stdout().lock());
    if let Some(m) = fmt {
        write_table(&mut out, &["count", "share", "value"], rows.iter().take(n).map(|(k, c)| {
            let p = if total > 0 { *c as f64 / total as f64 * 100.0 } else { 0.0 };
            vec![c.to_string(), format!("{p:.1}%"), k.clone()]
        }), m)?;
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

#[derive(Clone, Copy)]
enum Sep {
    Csv,
    Tsv,
    Md,
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

fn run_export(fields: Vec<String>, filters: Vec<jlf_core::Filter>, mode: Sep, input: &[String]) -> Result<(), color_eyre::Report> {
    let cols: Vec<Vec<String>> = fields.iter().map(|f| f.split('.').map(str::to_owned).collect()).collect();
    let mut stdout = io::BufWriter::with_capacity(64 * 1024, io::stdout().lock());

    // header
    match mode {
        Sep::Md => {
            writeln!(stdout, "| {} |", fields.join(" | "))?;
            writeln!(stdout, "| {} |", fields.iter().map(|_| "---").collect::<Vec<_>>().join(" | "))?;
        }
        Sep::Tsv => writeln!(stdout, "{}", fields.join("\t"))?,
        Sep::Csv => writeln!(stdout, "{}", fields.iter().map(|f| esc(f, mode)).collect::<Vec<_>>().join(","))?,
    }

    let mut buf = open_input(input)?;
    let mut line = String::new();
    let mut row = String::new();
    while buf.read_line(&mut line)? != 0 {
        if !line.trim().is_empty() {
            let mut json = Json::Null;
            if json.parse_replace(&line).is_ok()
                && (filters.is_empty() || jlf_core::matches_all(&filters, &json))
            {
                row.clear();
                for (i, p) in cols.iter().enumerate() {
                    if i > 0 {
                        row.push_str(match mode {
                            Sep::Csv => ",",
                            Sep::Tsv => "\t",
                            Sep::Md => " | ",
                        });
                    }
                    let v = resolve(&json, p);
                    row.push_str(&esc(scalar(v).unwrap_or(""), mode));
                }
                match mode {
                    Sep::Md => writeln!(stdout, "| {row} |")?,
                    _ => writeln!(stdout, "{row}")?,
                }
            }
        }
        line.clear();
    }
    stdout.flush()?;
    Ok(())
}

fn esc(s: &str, mode: Sep) -> String {
    match mode {
        Sep::Csv => {
            if s.contains([',', '"', '\n', '\r']) {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s.to_owned()
            }
        }
        Sep::Tsv => s.replace(['\t', '\n', '\r'], " "),
        Sep::Md => s.replace('|', "\\|").replace('\n', "<br>"),
    }
}
