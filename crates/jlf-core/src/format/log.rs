use core::cmp::Ordering;
use core::fmt;

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
            // Walk an `if / else if… / else` chain, running the first branch
            // whose condition holds. `matched` stays set once any branch wins so
            // later `else if`/`else` arms are skipped. When the whole block is
            // itself skipped, every branch is suppressed.
            let mut matched = !skip && test_cond(cond, args, *i, json, cur, used_fields);
            let mut should_run = matched;

            piece_i += 1;
            while piece_i < pieces.len() {
                match &pieces[piece_i] {
                    Piece::ElseCond(cond, i) => {
                        should_run =
                            !skip && !matched && test_cond(cond, args, *i, json, cur, used_fields);
                        matched |= should_run;
                        piece_i += 1;
                    }
                    Piece::Else => {
                        should_run = !skip && !matched;
                        matched = true;
                        piece_i += 1;
                    }
                    _ => {}
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
        MatchStart(subj) => {
            let end = find_match_end(pieces, piece_i);
            if !skip {
                // Bind the subject to `$value` and render the arm chain in
                // between; the arms (an if/else-if chain over `$value`) pick the
                // first matching one. Marking the subject consumed keeps it out
                // of a later `${..}` rest dump.
                let (field_options, _) = &args[*subj];
                let value = resolve_subject(json, field_options, cur, used_fields);
                let binding = Binding {
                    key: BindKey::Str(""),
                    value,
                };
                let mut i = piece_i + 1;
                while i < end {
                    i = write_piece(
                        f,
                        pieces,
                        i,
                        args,
                        json,
                        cols,
                        Some(&binding),
                        false,
                        used_fields,
                    )?;
                }
            }
            return Ok(end + 1);
        }
        MatchEnd => {}
    }

    Ok(piece_i + 1)
}

/// Index of the `MatchEnd` matching the `MatchStart` at `start` (nesting-aware).
fn find_match_end(pieces: &[Piece], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start + 1;
    while i < pieces.len() {
        match &pieces[i] {
            Piece::MatchStart(_) => depth += 1,
            Piece::MatchEnd => {
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

/// Resolve a `${match}` subject to its value. A plain path is looked up in the
/// record (and recorded in `used_fields` so a later `${..}` rest dump skips it);
/// `$value`/`${value.sub}` resolve against the enclosing repetition/match binding
/// so a match can dispatch on a loop entry. The first non-null option wins.
fn resolve_subject<'a>(
    json: &'a Json<'a>,
    field_options: &'a [Field],
    cur: Option<&Binding<'a>>,
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> &'a Json<'a> {
    let mut last = json;
    for field in field_options {
        last = match field {
            Field::ColValue => cur.map_or(json, |c| c.value),
            Field::ColValuePath(names) => cur.map_or(json, |c| walk(c.value, names)),
            _ => resolve_field(json, field),
        };
        if !last.is_null() {
            if matches!(field, Field::Names(_)) {
                used_fields.push(field);
            }
            return last;
        }
    }
    last
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
            // The base path of this iteration, so an object key already shown by
            // an earlier `${base.key}` reference is skipped here (like `${..}`).
            let base: &[FieldType] = match src {
                RepSource::Path(Field::Names(names)) => names,
                _ => &[],
            };
            if let Some(obj) = target.as_object() {
                for (k, v) in obj.iter() {
                    if key_consumed(used_fields, base, k) {
                        continue;
                    }
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
    let mut i = 0;
    while i < names.len() {
        match &names[i] {
            FieldType::Index(idx) => {
                val = val.get_i(*idx);
                i += 1;
            }
            FieldType::Name(n) => {
                let direct = val.get(n);
                if !direct.is_null() {
                    val = direct;
                    i += 1;
                    continue;
                }
                // Not a nested key: the remaining name tokens may be a single
                // flattened dotted key (e.g. tracing's `log.file` stored as one
                // literal key). Try joining consecutive names with '.', keeping
                // the longest match so the most specific key wins. Nested lookup
                // is tried first above, so real nesting still takes precedence.
                let mut joined = n.clone();
                let mut best: Option<(String, usize)> = None;
                let mut j = i + 1;
                while let Some(FieldType::Name(m)) = names.get(j) {
                    joined.push('.');
                    joined.push_str(m);
                    if !val.get(&joined).is_null() {
                        best = Some((joined.clone(), j + 1));
                    }
                    j += 1;
                }
                match best {
                    Some((key, end)) => {
                        val = val.get(&key);
                        i = end;
                    }
                    None => return direct,
                }
            }
        }
    }
    val
}

/// True when `base + key` was already consumed by an earlier field reference, so
/// a repetition over `base` should skip that entry (e.g. `${fields.message}` up
/// top drops `message` from a later `$fields( … )`). The trailing tokens after
/// `base` are joined with '.', so a flattened key like `log.file` (referenced as
/// `${fields.log.file}`) is also recognised and skipped.
fn key_consumed(used_fields: &SmallVec<[&Field; 5]>, base: &[FieldType], key: &str) -> bool {
    with_excluded(used_fields, |excluded| {
        excluded.iter().any(|path| {
            path.len() > base.len()
                && base.iter().zip(path.iter()).all(|(b, p)| match (b, p) {
                    (FieldType::Name(n), PathToken::Name(x)) => x == n,
                    (FieldType::Index(i), PathToken::Index(x)) => x == i,
                    _ => false,
                })
                && join_names_eq(&path[base.len()..], key)
        })
    })
}

/// Whether the `Name` tokens of `rest` joined with '.' equal `key` (and there
/// are no `Index` tokens) — recognising a flattened dotted key like `log.file`.
fn join_names_eq(rest: &[PathToken<'_>], key: &str) -> bool {
    let mut joined = String::new();
    for (i, tok) in rest.iter().enumerate() {
        match tok {
            PathToken::Name(n) => {
                if i > 0 {
                    joined.push('.');
                }
                joined.push_str(n);
            }
            PathToken::Index(_) => return false,
        }
    }
    joined == key
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
                let val = walk(json, names);
                if !val.is_null() {
                    return false; // this fallback renders; rest isn't reached
                }
            }
        }
    }
    false
}

fn test_cond<'a>(
    cond: &Cond,
    args: &[Arg],
    i: usize,
    json: &'a Json<'a>,
    cur: Option<&Binding<'a>>,
    used_fields: &SmallVec<[&'a Field; 5]>,
) -> bool {
    if let Cond::IfConfig(b) = cond {
        return *b;
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
                    BindKey::Str(s) => *cond == Cond::Has || !s.is_empty(),
                    BindKey::Index(_) => true,
                })
                .unwrap_or(false),
            Field::Rest => {
                // For `key`, `rest` is the base object and always exists. For
                // `if`, it's truthy only when there are unused fields left.
                if *cond == Cond::Has {
                    true
                } else {
                    with_excluded(used_fields, |excluded| json.has_rest_content(excluded))
                }
            }
            Field::Names(names) => {
                let val = walk(json, names);
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

fn test_cond2(cond: &Cond, json: &Json<'_>) -> bool {
    if json.is_null() {
        return false;
    }

    match cond {
        Cond::Has => true,
        Cond::Cmp(op, rhs) => compare_scalar(json, *op, rhs),
        Cond::Arm(tests) => match_arm(json, tests),
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
        Cond::IfConfig(_) => unreachable!("checked above"),
    }
}

/// Evaluate `field OP literal`. Compares numerically when both sides parse as
/// numbers, otherwise lexicographically. Objects/arrays never match.
fn compare_scalar(json: &Json<'_>, op: CmpOp, rhs: &str) -> bool {
    let Some(lhs) = json.as_str().or_else(|| json.as_value()) else {
        return false;
    };
    let ord = match (lhs.parse::<f64>(), rhs.parse::<f64>()) {
        (Ok(a), Ok(b)) => a.partial_cmp(&b),
        _ => Some(lhs.cmp(rhs)),
    };
    let Some(ord) = ord else {
        return false; // NaN — no ordering
    };
    match op {
        CmpOp::Eq => ord == Ordering::Equal,
        CmpOp::Ne => ord != Ordering::Equal,
        CmpOp::Gt => ord == Ordering::Greater,
        CmpOp::Ge => ord != Ordering::Less,
        CmpOp::Lt => ord == Ordering::Less,
        CmpOp::Le => ord != Ordering::Greater,
    }
}

/// Evaluate a `${match}` arm against the (non-null) subject: the wildcard (empty
/// list) matches any present value; otherwise any listed test may match.
fn match_arm(json: &Json<'_>, tests: &[ArmTest]) -> bool {
    if tests.is_empty() {
        return true; // `_`: the caller already excluded null subjects
    }
    tests.iter().any(|t| match t {
        ArmTest::Cmp(op, rhs) => compare_scalar(json, *op, rhs),
        ArmTest::Range {
            lo,
            hi,
            hi_inclusive,
        } => json
            .as_str()
            .or_else(|| json.as_value())
            .and_then(|s| s.parse::<f64>().ok())
            .is_some_and(|v| {
                lo.is_none_or(|lo| v >= lo)
                    && hi.is_none_or(|hi| if *hi_inclusive { v <= hi } else { v < hi })
            }),
    })
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
                val = walk(json, names);

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
        is_json: _,
        indent,
        optional: _,
        escape,
        markup_styles: json_styles,
    } = format;
    let indent = *indent;
    let escape = *escape;

    if indent > 0 {
        write!(f, "{:indent$}", "", indent = indent)?;
    }

    if let Some(val) = json.as_str() {
        if escape != Escape::None {
            write_escaped(f, escape, val)?;
        } else if let Some(style) = style {
            write!(f, "{}", val.style(*style))?;
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
        if escape != Escape::None {
            // A table/markup cell must stay on one line and be quoted for its
            // dialect, so nested objects/arrays render as compact JSON (no
            // ANSI) and then get escaped — never pretty-printed across rows.
            write_escaped(f, escape, &json.to_string())?;
        } else if *compact {
            if style.is_some() {
                write!(f, "{}", json.styled(*json_styles))?;
            } else {
                write!(f, "{}", json)?;
            }
        } else if style.is_some() {
            write!(f, "{:?}", json.indented(indent).styled(*json_styles))?;
        } else {
            write!(f, "{:?}", json.indented(indent))?;
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

