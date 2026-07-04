use core::fmt;

use owo_colors::AnsiColors;
pub use owo_colors::OwoColorize as Colorize;

use super::*;
use crate::json::PathToken;
use crate::Json;

// used for displaying the formatted log to output
pub struct FormattedLog<'a> {
    pub(super) formatter: &'a Formatter,
    pub(super) json: &'a Json<'a>,
}

/// The current entry bound inside a `$( … )` repetition: `$key`/`$value`.
struct Binding<'a> {
    key: BindKey<'a>,
    value: &'a Json<'a>,
}

enum BindKey<'a> {
    Str(&'a str),
    Index(usize),
}

impl fmt::Display for FormattedLog<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_fmt(f)
    }
}

impl FormattedLog<'_> {
    pub fn write_fmt(&self, f: &mut impl fmt::Write) -> Result<(), fmt::Error> {
        let Self {
            formatter:
                Formatter {
                    pieces,
                    args,
                    has_optional,
                    columns,
                },
            json,
        } = self;

        let mut used_fields = SmallVec::new();

        // Plain templates write straight through. Only when an optional
        // `{?field}` is present do we wrap the writer to defer whitespace so an
        // empty optional can collapse one adjacent space.
        if *has_optional {
            let mut w = Trimmer::new(f);
            render(&mut w, pieces, args, json, columns, &mut used_fields)?;
            w.flush_ws()?;
        } else {
            let mut w = Plain(f);
            render(&mut w, pieces, args, json, columns, &mut used_fields)?;
        }

        Ok(())
    }
}

fn render<'a>(
    f: &mut impl Write2,
    pieces: &'a Vec<Piece>,
    args: &'a Vec<Arg>,
    json: &'a Json<'a>,
    cols: &'a [Column],
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> fmt::Result {
    let mut piece_i = 0;
    while piece_i < pieces.len() {
        piece_i = write_piece(f, pieces, piece_i, args, json, cols, None, false, used_fields)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_piece<'a>(
    f: &mut impl Write2,
    pieces: &'a Vec<Piece>,
    mut piece_i: usize,
    args: &'a Vec<Arg>,
    json: &'a Json<'a>,
    cols: &'a [Column],
    cur: Option<&Binding<'a>>,
    skip: bool,
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> Result<usize, fmt::Error> {
    use Piece::*;

    match &pieces[piece_i] {
        Literal(literal) => {
            if !skip {
                write!(f, "{}", literal)?
            }
        }
        Arg(i) => {
            if !skip {
                let arg = &args[*i];
                if arg.1.optional {
                    // Render the optional field aside; if it produced nothing,
                    // collapse a neighbouring space instead of emitting it. An
                    // optional rest (`{?..}`) with no leftover fields counts as
                    // empty too (so it doesn't print a bare `{}`/`[]`).
                    let mut scratch = String::new();
                    let empty = rest_arg_without_content(arg, json, used_fields) || {
                        write_arg(&mut scratch, arg, json, cur, used_fields)?;
                        scratch.is_empty()
                    };
                    if empty {
                        f.collapse_ws();
                    } else {
                        f.write_str(&scratch)?;
                    }
                } else {
                    write_arg(f, arg, json, cur, used_fields)?
                }
            }
        }
        RepStart(src, sep, op) => {
            let end = find_rep_end(pieces, piece_i);
            if !skip {
                render_rep(
                    f, pieces, piece_i, end, src, sep, *op, args, json, cols, used_fields,
                )?;
            }
            return Ok(end + 1);
        }
        RepEnd => {}
        CondStart(cond, i) => {
            let cond_matched = !skip && test_cond(*cond, args, *i, json, cur, used_fields);
            let mut should_run = cond_matched;
            let mut else_cond_matched = false;

            piece_i += 1;
            while piece_i < pieces.len() {
                if let Piece::ElseCond(cond, i) = pieces[piece_i] {
                    if !skip && !cond_matched && !else_cond_matched {
                        should_run = test_cond(cond, args, i, json, cur, used_fields);
                        else_cond_matched = true;
                    } else {
                        should_run = false;
                    }

                    piece_i += 1;
                } else if let Piece::Else = pieces[piece_i] {
                    if !skip && !should_run && !else_cond_matched {
                        should_run = true;
                        else_cond_matched = true;
                    } else {
                        should_run = false;
                    }

                    piece_i += 1;
                }

                if let Piece::CondEnd = pieces[piece_i] {
                    break;
                }

                piece_i =
                    write_piece(f, pieces, piece_i, args, json, cols, cur, !should_run, used_fields)?;
            }
        }
        // Handled in the CondStart case above
        ElseCond(..) | Else | CondEnd => {}
    }

    Ok(piece_i + 1)
}

/// Index of the `RepEnd` matching the `RepStart` at `start` (nesting-aware).
fn find_rep_end(pieces: &[Piece], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start + 1;
    while i < pieces.len() {
        match &pieces[i] {
            Piece::RepStart(..) => depth += 1,
            Piece::RepEnd => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            _ => {}
        }
        i += 1;
    }
    pieces.len().saturating_sub(1)
}

/// Render a `$( … )` repetition: build the bindings from `src`, render the body
/// once per binding, and write `sep` between iterations (never trailing).
#[allow(clippy::too_many_arguments)]
fn render_rep<'a>(
    f: &mut impl Write2,
    pieces: &'a Vec<Piece>,
    start: usize,
    end: usize,
    src: &RepSource,
    sep: &str,
    op: RepOp,
    args: &'a Vec<Arg>,
    json: &'a Json<'a>,
    cols: &'a [Column],
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> fmt::Result {
    let mut bindings: Vec<Binding<'a>> = Vec::new();
    match src {
        RepSource::Columns => {
            for col in cols {
                bindings.push(Binding {
                    key: BindKey::Str(&col.name),
                    value: resolve_field(json, &col.field),
                });
            }
        }
        // `$( … )` iterates the record itself; `$path( … )` a sub-object/array.
        RepSource::Record | RepSource::Path(_) => {
            let target = match src {
                RepSource::Path(field) => resolve_field(json, field),
                _ => json,
            };
            if let Some(obj) = target.as_object() {
                for (k, v) in obj.iter() {
                    bindings.push(Binding {
                        key: BindKey::Str(k),
                        value: v,
                    });
                }
            } else if let Some(arr) = target.as_array() {
                for (idx, v) in arr.iter().enumerate() {
                    bindings.push(Binding {
                        key: BindKey::Index(idx),
                        value: v,
                    });
                }
            }
        }
    }

    // `?` renders at most one iteration; `*`/`+` render them all.
    let count = if op == RepOp::Question {
        bindings.len().min(1)
    } else {
        bindings.len()
    };

    for (n, b) in bindings.iter().take(count).enumerate() {
        if n > 0 {
            f.write_str(sep)?;
        }
        let mut i = start + 1;
        while i < end {
            i = write_piece(f, pieces, i, args, json, cols, Some(b), false, used_fields)?;
        }
    }

    Ok(())
}

/// Walk a name path from a `Json` node (used for `${value.path}` in a rep).
fn walk<'a>(mut val: &'a Json<'a>, names: &FieldNames) -> &'a Json<'a> {
    for t in names {
        val = match t {
            FieldType::Name(n) => val.get(n),
            FieldType::Index(i) => val.get_i(*i),
        };
    }
    val
}

/// Resolve a field accessor to the pointed-at `Json` (no rest handling).
fn resolve_field<'a>(json: &'a Json<'a>, field: &Field) -> &'a Json<'a> {
    match field {
        Field::Whole | Field::Rest | Field::ColValue | Field::ColKey | Field::ColValuePath(_) => {
            json
        }
        Field::Names(names) => walk(json, names),
    }
}

/// True when an optional arg's selected field is the rest (`..`) and there are no
/// leftover fields to show — so `{?..}` renders empty and collapses its space
/// rather than printing a bare `{}`/`[]`. A present earlier fallback (`{?a|..}`
/// with `a` set) returns false, since rest is never reached.
fn rest_arg_without_content<'a>(
    (field_options, _): &'a Arg,
    json: &'a Json<'a>,
    used_fields: &SmallVec<[&'a Field; 5]>,
) -> bool {
    for field in field_options {
        match field {
            Field::Whole => return false,
            Field::ColValue | Field::ColKey | Field::ColValuePath(_) => return false,
            Field::Rest => {
                return !with_excluded(used_fields, |excluded| json.has_rest_content(excluded));
            }
            Field::Names(names) => {
                let mut val = json;
                for arg in names {
                    val = match arg {
                        FieldType::Name(name) => val.get(name),
                        FieldType::Index(index) => val.get_i(*index),
                    };
                }
                if !val.is_null() {
                    return false; // this fallback renders; rest isn't reached
                }
            }
        }
    }
    false
}

fn test_cond<'a>(
    cond: Cond,
    args: &[Arg],
    i: usize,
    json: &'a Json<'a>,
    cur: Option<&Binding<'a>>,
    used_fields: &SmallVec<[&'a Field; 5]>,
) -> bool {
    if let Cond::IfConfig(b) = cond {
        return b;
    }

    let (field_options, _) = &args[i];
    for field in field_options {
        let matched = match field {
            Field::Whole => test_cond2(cond, json),
            Field::ColValue => cur.map(|c| test_cond2(cond, c.value)).unwrap_or(false),
            Field::ColValuePath(names) => cur
                .map(|c| test_cond2(cond, walk(c.value, names)))
                .unwrap_or(false),
            Field::ColKey => cur
                .map(|c| match c.key {
                    BindKey::Str(s) => cond == Cond::Key || !s.is_empty(),
                    BindKey::Index(_) => true,
                })
                .unwrap_or(false),
            Field::Rest => {
                // For `key`, `rest` is the base object and always exists. For
                // `if`, it's truthy only when there are unused fields left.
                if cond == Cond::Key {
                    true
                } else {
                    with_excluded(used_fields, |excluded| json.has_rest_content(excluded))
                }
            }
            Field::Names(names) => {
                let mut val = json;
                for arg in names {
                    match arg {
                        FieldType::Name(name) => val = val.get(name),
                        FieldType::Index(index) => val = val.get_i(*index),
                    }
                }
                test_cond2(cond, val)
            }
        };

        // A fallback list (`a|b|c`) is satisfied as soon as one option is:
        // `#key` matches the first option that exists, `#if` the first truthy
        // one. Earlier present-but-falsey options must not short-circuit `#if`.
        if matched {
            return true;
        }
    }

    false
}

fn test_cond2(cond: Cond, json: &Json<'_>) -> bool {
    if json.is_null() {
        return false;
    }

    match cond {
        Cond::Key => true,
        Cond::If => {
            if json.is_array() || json.is_object() {
                !json.is_empty()
            } else if let Some(json) = json.as_str() {
                !json.is_empty()
            } else if let Some(json) = json.as_value() {
                !(json == "false"
                    || json == "0"
                    || json == "-0"
                    || json == "0n"
                    || json == "undefined"
                    || json == "NaN")
            } else {
                unreachable!("all cases checked")
            }
        }
        _ => unreachable!("checked above"),
    }
}

fn write_arg<'a>(
    f: &mut impl fmt::Write,
    (field_options, format): &'a (FieldOptions, Format),
    json: &'a Json<'a>,
    cur: Option<&Binding<'a>>,
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> fmt::Result {
    let mut val = &Json::Null;

    for field in field_options {
        val = json;

        match field {
            Field::Whole => {
                return write_arg2(f, format, json);
            }
            Field::Rest => {
                return write_rest(f, format, json, used_fields);
            }
            Field::ColValue => {
                return match cur {
                    Some(c) => write_arg2(f, format, c.value),
                    None => Ok(()),
                };
            }
            Field::ColValuePath(names) => {
                return match cur {
                    Some(c) => write_arg2(f, format, walk(c.value, names)),
                    None => Ok(()),
                };
            }
            Field::ColKey => {
                return match cur {
                    Some(c) => match c.key {
                        BindKey::Str(s) => write_scalar_str(f, format, s),
                        BindKey::Index(idx) => write_scalar_str(f, format, &idx.to_string()),
                    },
                    None => Ok(()),
                };
            }
            Field::Names(names) => {
                for arg in names {
                    match arg {
                        FieldType::Name(name) => {
                            val = val.get(name);
                        }
                        FieldType::Index(index) => {
                            val = val.get_i(*index);
                        }
                    }
                }

                if !val.is_null() {
                    used_fields.push(field);
                    break;
                }
            }
        }
    }

    write_arg2(f, format, val)
}

/// Write a plain string value with the arg's escape or style applied (used for
/// `$key`, which is a bare string/index rather than a `Json` node).
fn write_scalar_str(f: &mut impl fmt::Write, format: &Format, val: &str) -> fmt::Result {
    if format.escape != Escape::None {
        return write_escaped(f, format.escape, val);
    }
    if let Some(style) = format.style {
        write!(f, "{}", val.style(style))
    } else {
        f.write_str(val)
    }
}

fn write_arg2(f: &mut impl fmt::Write, format: &Format, json: &Json<'_>) -> fmt::Result {
    let Format {
        style,
        compact,
        is_json,
        is_level,
        indent,
        optional: _,
        escape,
        markup_styles: json_styles,
    } = format;
    let indent = *indent;
    let is_level = *is_level;
    let escape = *escape;

    if indent > 0 {
        write!(f, "{:indent$}", "", indent = indent)?;
    }

    if let Some(val) = json.as_str() {
        if escape != Escape::None {
            write_escaped(f, escape, val)?;
        } else if let Some(style) = style {
            if is_level {
                match val {
                    "TRACE" | "trace" => write!(
                        f,
                        "{}",
                        val.style((*style).color(AnsiColors::Cyan).dimmed())
                    )?,
                    "DEBUG" | "debug" => {
                        write!(f, "{}", val.style((*style).color(AnsiColors::Green)))?
                    }
                    "INFO" | "info" => {
                        write!(f, " {}", val.style((*style).color(AnsiColors::Cyan)))?
                    }
                    "WARN" | "warn" => {
                        write!(f, " {}", val.style((*style).color(AnsiColors::Yellow)))?
                    }
                    "ERROR" | "error" => {
                        write!(f, "{}", val.style((*style).color(AnsiColors::Red)))?
                    }
                    _ => write!(f, "{}", val.style(*style))?,
                }
            } else {
                write!(f, "{}", val.style(*style))?;
            }
        } else {
            write!(f, "{}", val)?;
        }
    } else if let Some(val) = json.as_value() {
        if escape != Escape::None {
            write_escaped(f, escape, val)?;
        } else if let Some(style) = style.as_ref() {
            write!(f, "{}", val.style(*style))?;
        } else {
            write!(f, "{}", val)?;
        }
    } else if json.is_object() || json.is_array() {
        match (is_json, compact) {
            (true, true) => {
                if style.is_some() {
                    write!(f, "{}", json.styled(*json_styles))?;
                } else {
                    write!(f, "{}", json)?;
                }
            }
            (true, false) => {
                if style.is_some() {
                    write!(f, "{:?}", json.indented(indent).styled(*json_styles))?;
                } else {
                    write!(f, "{:?}", json.indented(indent))?;
                }
            }
            (false, true) => {
                if style.is_some() {
                    write!(f, "{}", json.styled(*json_styles))?;
                } else {
                    write!(f, "{}", json)?;
                }
            }
            (false, false) => {
                if style.is_some() {
                    write!(f, "{:?}", json.indented(indent).styled(*json_styles))?;
                } else {
                    write!(f, "{:?}", json.indented(indent))?;
                }
            }
        }
    }

    Ok(())
}

/// Builds the list of already-consumed field paths (as `PathToken` slices) from
/// `used_fields`, so they can be skipped when rendering the rest object.
fn build_excluded<'a>(
    used_fields: &SmallVec<[&'a Field; 5]>,
) -> SmallVec<[SmallVec<[PathToken<'a>; 2]>; 5]> {
    let mut paths = SmallVec::new();

    for field in used_fields.iter() {
        if let Field::Names(names) = field {
            let tokens = names
                .iter()
                .map(|t| match t {
                    FieldType::Name(name) => PathToken::Name(name.as_str()),
                    FieldType::Index(index) => PathToken::Index(*index),
                })
                .collect();
            paths.push(tokens);
        }
    }

    paths
}

/// Builds the excluded-path slice view from `used_fields` and runs `f` with it.
fn with_excluded<R>(
    used_fields: &SmallVec<[&Field; 5]>,
    f: impl FnOnce(&[&[PathToken]]) -> R,
) -> R {
    let paths = build_excluded(used_fields);
    let excluded: SmallVec<[&[PathToken]; 5]> = paths.iter().map(|p| p.as_slice()).collect();
    f(&excluded)
}

/// Write `s` with the given escape applied.
fn write_escaped(f: &mut impl fmt::Write, escape: Escape, s: &str) -> fmt::Result {
    match escape {
        Escape::None => f.write_str(s),
        Escape::Html => {
            for c in s.chars() {
                match c {
                    '&' => f.write_str("&amp;")?,
                    '<' => f.write_str("&lt;")?,
                    '>' => f.write_str("&gt;")?,
                    '"' => f.write_str("&quot;")?,
                    '\'' => f.write_str("&#39;")?,
                    _ => f.write_char(c)?,
                }
            }
            Ok(())
        }
        Escape::Csv => {
            if s.contains('"') || s.contains('\n') || s.contains('\r') || s.contains(',') {
                f.write_char('"')?;
                for c in s.chars() {
                    if c == '"' {
                        f.write_str("\"\"")?;
                    } else {
                        f.write_char(c)?;
                    }
                }
                f.write_char('"')
            } else {
                f.write_str(s)
            }
        }
        Escape::Tsv => {
            for c in s.chars() {
                match c {
                    '\t' | '\n' | '\r' => f.write_char(' ')?,
                    _ => f.write_char(c)?,
                }
            }
            Ok(())
        }
        Escape::Md => {
            for c in s.chars() {
                match c {
                    '|' => f.write_str("\\|")?,
                    '\n' => f.write_str("<br>")?,
                    _ => f.write_char(c)?,
                }
            }
            Ok(())
        }
    }
}

/// Renders the rest object (`{..}`) as a filtered view of `json`, skipping the
/// fields already consumed by earlier args. Mirrors the object/array branch of
/// [`write_arg2`] but never clones the underlying `Json`.
fn write_rest(
    f: &mut impl fmt::Write,
    format: &Format,
    json: &Json,
    used_fields: &SmallVec<[&Field; 5]>,
) -> fmt::Result {
    // Scalars can't have "rest" fields removed; fall back to the normal path.
    if !(json.is_object() || json.is_array()) {
        return write_arg2(f, format, json);
    }

    let Format {
        style,
        compact,
        indent,
        markup_styles,
        ..
    } = format;
    let indent = *indent;

    if indent > 0 {
        write!(f, "{:indent$}", "", indent = indent)?;
    }

    let styles = style.map(|_| *markup_styles);
    with_excluded(used_fields, |excluded| {
        let view = RestView {
            json,
            excluded,
            indent,
            styles,
        };

        if *compact {
            write!(f, "{}", view)
        } else {
            write!(f, "{:?}", view)
        }
    })
}

/// A `Display`/`Debug` wrapper that renders the rest object while skipping the
/// `excluded` field paths. `Display` => compact, `Debug` => pretty.
struct RestView<'a> {
    json: &'a Json<'a>,
    excluded: &'a [&'a [PathToken<'a>]],
    indent: usize,
    styles: Option<MarkupStyles>,
}

impl fmt::Display for RestView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.json.fmt_rest(f, self.excluded, None, &self.styles)
    }
}

impl fmt::Debug for RestView<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.json
            .fmt_rest(f, self.excluded, Some(self.indent), &self.styles)
    }
}

/// A writer that can collapse one whitespace separator next to an empty
/// `{?field}`. Plain writers treat the collapse hooks as no-ops, so non-optional
/// templates render with zero extra work.
trait Write2: fmt::Write {
    /// An optional field rendered empty: collapse one adjacent space.
    fn collapse_ws(&mut self) {}
    /// Emit any deferred trailing whitespace.
    fn flush_ws(&mut self) -> fmt::Result {
        Ok(())
    }
}

/// Pass-through writer for templates without any `{?field}`.
struct Plain<'a, W: fmt::Write>(&'a mut W);

impl<W: fmt::Write> fmt::Write for Plain<'_, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_str(s)
    }
}

impl<W: fmt::Write> Write2 for Plain<'_, W> {}

/// Whitespace-deferring writer used when a template contains `{?field}`.
///
/// Trailing whitespace is buffered rather than written immediately, so when an
/// empty optional asks to collapse a space we can either drop that buffered run
/// (the space *before* the field) or skip the next run (the space *after* it) —
/// collapsing exactly one separator so an absent field leaves no stray gap.
struct Trimmer<'a, W: fmt::Write> {
    inner: &'a mut W,
    pending: String,
    skip_leading_ws: bool,
}

impl<'a, W: fmt::Write> Trimmer<'a, W> {
    fn new(inner: &'a mut W) -> Self {
        Self {
            inner,
            pending: String::new(),
            skip_leading_ws: false,
        }
    }
}

impl<W: fmt::Write> fmt::Write for Trimmer<'_, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Collapsible separators include newlines, not just spaces/tabs, so an
        // empty optional can absorb a line break (e.g. the `\n` before `{?@data}`).
        const WS: [char; 4] = [' ', '\t', '\n', '\r'];
        let mut s = s;
        if self.skip_leading_ws {
            let trimmed = s.trim_start_matches(WS);
            let stripped_some = trimmed.len() != s.len();
            s = trimmed;
            // Keep skipping only while the run is still all whitespace; stop once
            // real content (this or a later write) appears.
            if !stripped_some || !s.is_empty() {
                self.skip_leading_ws = false;
            }
        }
        if s.is_empty() {
            return Ok(());
        }
        let core_len = s.trim_end_matches(WS).len();
        let (core, trail) = s.split_at(core_len);
        if !core.is_empty() {
            if !self.pending.is_empty() {
                self.inner.write_str(&self.pending)?;
                self.pending.clear();
            }
            self.inner.write_str(core)?;
        }
        self.pending.push_str(trail);
        Ok(())
    }
}

impl<W: fmt::Write> Write2 for Trimmer<'_, W> {
    fn collapse_ws(&mut self) {
        if self.pending.is_empty() {
            self.skip_leading_ws = true;
        } else {
            self.pending.clear();
        }
    }

    fn flush_ws(&mut self) -> fmt::Result {
        if !self.pending.is_empty() {
            self.inner.write_str(&self.pending)?;
            self.pending.clear();
        }
        Ok(())
    }
}
