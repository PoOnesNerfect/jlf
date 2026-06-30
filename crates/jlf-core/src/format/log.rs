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

impl fmt::Display for FormattedLog<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.write_fmt(f) }
}

impl FormattedLog<'_> {
    pub fn write_fmt(&self, f: &mut impl fmt::Write) -> Result<(), fmt::Error> {
        let Self {
            formatter:
                Formatter {
                    pieces,
                    args,
                    has_optional,
                },
            json,
        } = self;

        let mut used_fields = SmallVec::new();

        // Plain templates write straight through. Only when an optional
        // `{?field}` is present do we wrap the writer to defer whitespace so an
        // empty optional can collapse one adjacent space.
        if *has_optional {
            let mut w = Trimmer::new(f);
            render(&mut w, pieces, args, json, &mut used_fields)?;
            w.flush_ws()?;
        } else {
            let mut w = Plain(f);
            render(&mut w, pieces, args, json, &mut used_fields)?;
        }

        Ok(())
    }
}

fn render<'a>(
    f: &mut impl Write2,
    pieces: &'a Vec<Piece>,
    args: &'a Vec<Arg>,
    json: &'a Json<'a>,
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> fmt::Result {
    let mut piece_i = 0;
    while piece_i < pieces.len() {
        piece_i = write_piece(f, pieces, piece_i, args, json, false, used_fields)?;
    }
    Ok(())
}

fn write_piece<'a>(
    f: &mut impl Write2,
    pieces: &'a Vec<Piece>,
    mut piece_i: usize,
    args: &'a Vec<Arg>,
    json: &'a Json<'a>,
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
        Escaped(c) => {
            if !skip {
                write!(f, "{}", c)?
            }
        }
        Arg(i) => {
            if !skip {
                let arg = &args[*i];
                if arg.1.optional {
                    // Render the optional field aside; if it produced nothing,
                    // collapse a neighbouring space instead of emitting it.
                    let mut scratch = String::new();
                    write_arg(&mut scratch, arg, json, used_fields)?;
                    if scratch.is_empty() {
                        f.collapse_ws();
                    } else {
                        f.write_str(&scratch)?;
                    }
                } else {
                    write_arg(f, arg, json, used_fields)?
                }
            }
        }
        CondStart(cond, i) => {
            let cond_matched = !skip && test_cond(*cond, args, *i, json, used_fields);
            let mut should_run = cond_matched;
            let mut else_cond_matched = false;

            piece_i += 1;
            while piece_i < pieces.len() {
                if let Piece::ElseCond(cond, i) = pieces[piece_i] {
                    if !skip && !cond_matched && !else_cond_matched {
                        should_run = test_cond(cond, args, i, json, used_fields);
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

                piece_i = write_piece(f, pieces, piece_i, args, json, !should_run, used_fields)?;
            }
        }
        // Handled in the IfStart case above
        ElseCond(..) | Else | CondEnd => {}
    }

    Ok(piece_i + 1)
}

fn test_cond<'a>(
    cond: Cond,
    args: &[Arg],
    i: usize,
    json: &'a Json<'a>,
    used_fields: &SmallVec<[&'a Field; 5]>,
) -> bool {
    if let Cond::IfConfig(b) = cond {
        return b;
    }

    let (field_options, _) = &args[i];
    for field in field_options {
        let matched = match field {
            Field::Whole => test_cond2(cond, json),
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

fn write_arg2(f: &mut impl fmt::Write, format: &Format, json: &Json<'_>) -> fmt::Result {
    let Format {
        style,
        compact,
        is_json,
        is_level,
        indent,
        optional: _,
        markup_styles: json_styles,
    } = format;
    let indent = *indent;
    let is_level = *is_level;

    if indent > 0 {
        write!(f, "{:indent$}", "", indent = indent)?;
    }

    if let Some(val) = json.as_str() {
        if let Some(style) = style {
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
        if let Some(style) = style.as_ref() {
            write!(f, "{}", val.style(*style))?;
        } else {
            write!(f, "{}", val)?;
        }
    } else if json.is_object() || json.is_array() {
        // TODO: Implement formatting for objects
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
///
/// `write_arg` only ever pushes `Field::Names` entries into `used_fields`, so
/// any other variant is ignored here.
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
/// The owned `PathToken` storage lives for the duration of the call.
fn with_excluded<R>(
    used_fields: &SmallVec<[&Field; 5]>,
    f: impl FnOnce(&[&[PathToken]]) -> R,
) -> R {
    let paths = build_excluded(used_fields);
    let excluded: SmallVec<[&[PathToken]; 5]> = paths.iter().map(|p| p.as_slice()).collect();
    f(&excluded)
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
        let mut s = s;
        if self.skip_leading_ws {
            let trimmed = s.trim_start_matches([' ', '\t']);
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
        let core_len = s.trim_end_matches([' ', '\t']).len();
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
