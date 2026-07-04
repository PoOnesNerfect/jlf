use std::num::ParseIntError;

use owo_colors::Style;
use smallvec::SmallVec;

use super::{Arg, Cond, Escape, Field, FieldOptions, FieldType, Format, Piece, RepOp, RepSource};
use crate::{
    colors::{parse_color, ParseColorError},
    json::MarkupStyles,
};

/// Parse a `$`-DSL template into render pieces + args. Plain text is literal;
/// `$` introduces interpolation. Forms:
///   `$name`             a bare field
///   `${ … }`            a field (path/fallbacks/modifiers) or a directive
///   `$( … )"sep"op`     repetition over the selected columns (`-f`/args)
///   `$path( … )"sep"op` repetition over the object/array at `path`
///   `$$`                a literal `$`
pub(super) fn crunch_input(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    input: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    let mut sc = Scanner {
        b: input.as_bytes(),
        i: 0,
        no_color,
        compact,
    };
    sc.scan(pieces, args, 0, false)?;
    absorb_line_prefixes(pieces);
    Ok(())
}

/// When a `$( … )` / `$path( … )` / `$path?( … )` block starts its own line in the
/// template, pull the preceding newline + indentation into the front of the
/// block's body. That way each block can be written on its own indented source
/// line for readability, while the line break renders per iteration (reps) or
/// once (conditionals) — not unconditionally before the block.
fn absorb_line_prefixes(pieces: &mut Vec<Piece>) {
    let mut i = 0;
    while i < pieces.len() {
        let is_block = matches!(pieces[i], Piece::RepStart(..) | Piece::CondStart(..));
        if is_block && i > 0 {
            if let Piece::Literal(prev) = &pieces[i - 1] {
                if let Some(cut) = line_prefix_start(prev) {
                    let prefix = prev[cut..].to_owned();
                    if let Piece::Literal(p) = &mut pieces[i - 1] {
                        p.truncate(cut);
                    }
                    pieces.insert(i + 1, Piece::Literal(prefix));
                }
            }
        }
        i += 1;
    }
}

/// Byte index where a trailing `\n[ \t]*` run begins (block sits at a line start),
/// or `None` if the literal doesn't end that way.
fn line_prefix_start(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut j = b.len();
    while j > 0 && (b[j - 1] == b' ' || b[j - 1] == b'\t') {
        j -= 1;
    }
    (j > 0 && b[j - 1] == b'\n').then(|| j - 1)
}

struct Scanner<'a> {
    b: &'a [u8],
    i: usize,
    no_color: bool,
    compact: bool,
}

impl Scanner<'_> {
    /// Scan into `pieces`. When `in_rep`, stop after the matching (paren-balanced)
    /// `)` and leave the cursor just past it.
    fn scan(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
        in_rep: bool,
    ) -> Result<(), FormatError> {
        let mut lit = String::new();
        let mut paren_depth: usize = 0;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.take_escape(&mut lit)?,
                b'$' => match self.b.get(self.i + 1).copied() {
                    Some(b'$') => {
                        lit.push('$');
                        self.i += 2;
                    }
                    Some(b'(') => {
                        flush_lit(pieces, &mut lit);
                        self.i += 2;
                        self.parse_rep(pieces, args, rep_depth, RepSource::Record)?;
                    }
                    Some(b'{') => {
                        flush_lit(pieces, &mut lit);
                        self.i += 2;
                        self.parse_brace(pieces, args, rep_depth)?;
                    }
                    Some(x) if is_ident_start(x) => {
                        flush_lit(pieces, &mut lit);
                        self.i += 1;
                        self.parse_bare(pieces, args, rep_depth)?;
                    }
                    _ => {
                        lit.push('$');
                        self.i += 1;
                    }
                },
                b')' if in_rep => {
                    if paren_depth == 0 {
                        flush_lit(pieces, &mut lit);
                        self.i += 1;
                        return Ok(());
                    }
                    paren_depth -= 1;
                    lit.push(')');
                    self.i += 1;
                }
                b'(' if in_rep => {
                    paren_depth += 1;
                    lit.push('(');
                    self.i += 1;
                }
                _ => {
                    let start = self.i;
                    while self.i < self.b.len() {
                        let c = self.b[self.i];
                        if c == b'\\' || c == b'$' || (in_rep && (c == b'(' || c == b')')) {
                            break;
                        }
                        self.i += 1;
                    }
                    lit.push_str(std::str::from_utf8(&self.b[start..self.i]).unwrap());
                }
            }
        }
        if in_rep {
            return Err(FormatError::UnclosedRep);
        }
        flush_lit(pieces, &mut lit);
        Ok(())
    }

    fn take_escape(&mut self, lit: &mut String) -> Result<(), FormatError> {
        self.i += 1;
        let Some(&e) = self.b.get(self.i) else {
            lit.push('\\');
            return Ok(());
        };
        self.i += 1;
        lit.push(match e {
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'\\' => '\\',
            b'$' => '$',
            b'(' => '(',
            b')' => ')',
            b'\'' => '\'',
            b'"' => '"',
            b'{' => '{',
            b'}' => '}',
            other => return Err(FormatError::UnknownCharEscape(other as char)),
        });
        Ok(())
    }

    /// After `$` + identifier start: read a path. If followed by `(` it's a
    /// `$path( … )` repetition source; otherwise a plain field.
    fn parse_bare(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
    ) -> Result<(), FormatError> {
        let start = self.i;
        while self.i < self.b.len() && is_path_byte(self.b[self.i]) {
            self.i += 1;
        }
        let name = std::str::from_utf8(&self.b[start..self.i]).unwrap();
        if self.b.get(self.i) == Some(&b'(') {
            self.i += 1;
            let src = match name {
                "cols" => RepSource::Columns,
                _ => RepSource::Path(parse_field(name)?),
            };
            return self.parse_rep(pieces, args, rep_depth, src);
        }
        // `$path?( … )` — a conditional block: render the body when `path` is
        // truthy (shorthand for `${if path} … ${/}`).
        if self.b.get(self.i) == Some(&b'?') && self.b.get(self.i + 1) == Some(&b'(') {
            self.i += 2;
            return self.parse_cond_block(pieces, args, rep_depth, name);
        }
        let mut fields = FieldOptions::new();
        push_field(&mut fields, name, rep_depth)?;
        args.push((fields, parse_format(None, self.no_color, self.compact)?));
        pieces.push(Piece::Arg(args.len() - 1));
        Ok(())
    }

    /// Parse a `$path?( body )` conditional block (cursor just past `?(`): emit
    /// `CondStart(If, path)` + body + `CondEnd`, so the body renders only when
    /// `path` is truthy.
    fn parse_cond_block(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
        path: &str,
    ) -> Result<(), FormatError> {
        let mut fo = FieldOptions::new();
        crunch_field_options(path, &mut fo)?;
        args.push((fo, parse_format(None, self.no_color, self.compact)?));
        pieces.push(Piece::CondStart(Cond::If, args.len() - 1));

        let mut body = Vec::new();
        self.scan(&mut body, args, rep_depth, true)?;
        trim_body_edges(&mut body);
        pieces.extend(body);
        pieces.push(Piece::CondEnd);
        Ok(())
    }

    /// Parse a `${ … }` group: a directive (if/key/config/else/end) or a field.
    fn parse_brace(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
    ) -> Result<(), FormatError> {
        let start = self.i;
        while self.i < self.b.len() && self.b[self.i] != b'}' {
            self.i += 1;
        }
        if self.i >= self.b.len() {
            return Err(FormatError::ClosingBrace);
        }
        let content = std::str::from_utf8(&self.b[start..self.i]).unwrap().trim();
        self.i += 1; // consume '}'

        if let Some(c) = content.strip_prefix("if ") {
            self.push_cond(pieces, args, c, Cond::If, false)
        } else if let Some(c) = content.strip_prefix("key ") {
            self.push_cond(pieces, args, c, Cond::Key, false)
        } else if let Some(c) = content.strip_prefix("config ") {
            let b = match c.trim() {
                "compact" => self.compact,
                "no_color" => self.no_color,
                other => {
                    return Err(FormatError::UnsupportedConfig {
                        config: other.to_owned(),
                    })
                }
            };
            pieces.push(Piece::CondStart(Cond::IfConfig(b), 0));
            Ok(())
        } else if content == "else" {
            pieces.push(Piece::Else);
            Ok(())
        } else if let Some(c) = content.strip_prefix("else if ") {
            self.push_cond(pieces, args, c, Cond::If, true)
        } else if let Some(c) = content.strip_prefix("else key ") {
            self.push_cond(pieces, args, c, Cond::Key, true)
        } else if content.starts_with('/') {
            pieces.push(Piece::CondEnd);
            Ok(())
        } else {
            self.push_arg(pieces, args, content, rep_depth)
        }
    }

    fn push_cond(
        &self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        content: &str,
        cond: Cond,
        is_else: bool,
    ) -> Result<(), FormatError> {
        let mut fo = FieldOptions::new();
        crunch_field_options(content.trim(), &mut fo)?;
        args.push((fo, parse_format(None, self.no_color, self.compact)?));
        let idx = args.len() - 1;
        pieces.push(if is_else {
            Piece::ElseCond(cond, idx)
        } else {
            Piece::CondStart(cond, idx)
        });
        Ok(())
    }

    fn push_arg(
        &self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        content: &str,
        rep_depth: usize,
    ) -> Result<(), FormatError> {
        let (name_part, mut format) = match content.split_once(':') {
            Some((n, styles)) => (n.trim(), parse_format(Some(styles), self.no_color, self.compact)?),
            None => (content, parse_format(None, self.no_color, self.compact)?),
        };
        let name_part = match name_part.strip_prefix('?') {
            Some(rest) => {
                format.optional = true;
                rest.trim()
            }
            None => name_part,
        };
        let mut fields = FieldOptions::new();
        push_field(&mut fields, name_part, rep_depth)?;
        args.push((fields, format));
        pieces.push(Piece::Arg(args.len() - 1));
        Ok(())
    }

    /// Parse a repetition body (cursor just past `(`), then its separator and
    /// operator; emit RepStart/body/RepEnd.
    fn parse_rep(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
        src: RepSource,
    ) -> Result<(), FormatError> {
        let mut body = Vec::new();
        self.scan(&mut body, args, rep_depth + 1, true)?;
        trim_body_edges(&mut body);
        let sep = self.read_separator()?;
        let op = self.read_op()?;
        pieces.push(Piece::RepStart(src, sep, op));
        pieces.extend(body);
        pieces.push(Piece::RepEnd);
        Ok(())
    }

    /// Read the separator that sits between `)` and the operator: an optional
    /// `"quoted"` string (spaces/escapes preserved) or a bare run up to the op.
    fn read_separator(&mut self) -> Result<String, FormatError> {
        match self.b.get(self.i) {
            None | Some(b'*') | Some(b'+') | Some(b'?') => Ok(String::new()),
            Some(b'"') => {
                self.i += 1;
                let mut bytes = Vec::new();
                loop {
                    let Some(&c) = self.b.get(self.i) else {
                        return Err(FormatError::UnterminatedSep);
                    };
                    self.i += 1;
                    match c {
                        b'"' => break,
                        b'\\' => {
                            let Some(&e) = self.b.get(self.i) else {
                                return Err(FormatError::UnterminatedSep);
                            };
                            self.i += 1;
                            bytes.push(match e {
                                b'n' => b'\n',
                                b't' => b'\t',
                                b'r' => b'\r',
                                other => other,
                            });
                        }
                        other => bytes.push(other),
                    }
                }
                String::from_utf8(bytes).map_err(|_| FormatError::UnterminatedSep)
            }
            Some(_) => {
                let start = self.i;
                while let Some(&c) = self.b.get(self.i) {
                    if c == b'*' || c == b'+' || c == b'?' {
                        break;
                    }
                    self.i += 1;
                }
                Ok(std::str::from_utf8(&self.b[start..self.i]).unwrap().to_owned())
            }
        }
    }

    fn read_op(&mut self) -> Result<RepOp, FormatError> {
        let op = match self.b.get(self.i) {
            Some(b'*') => RepOp::Star,
            Some(b'+') => RepOp::Plus,
            Some(b'?') => RepOp::Question,
            _ => return Err(FormatError::MissingRepOp),
        };
        self.i += 1;
        Ok(op)
    }
}

fn flush_lit(pieces: &mut Vec<Piece>, lit: &mut String) {
    if !lit.is_empty() {
        pieces.push(Piece::Literal(std::mem::take(lit)));
    }
}

/// Trim spaces/tabs on the inner edges of a repetition body, so `$( $key )`
/// reads as `$key`. Newlines are preserved, so a multiline rep body (each item
/// on its own indented line) keeps its layout.
fn trim_body_edges(body: &mut Vec<Piece>) {
    const SP: [char; 2] = [' ', '\t'];
    if let Some(Piece::Literal(s)) = body.first_mut() {
        let t = s.trim_start_matches(SP).to_owned();
        if t.is_empty() {
            body.remove(0);
        } else {
            *s = t;
        }
    }
    if let Some(Piece::Literal(s)) = body.last_mut() {
        let t = s.trim_end_matches(SP).to_owned();
        if t.is_empty() {
            body.pop();
        } else {
            *s = t;
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// Bytes allowed in a bare field path: identifiers plus `.`/`[`/`]` for nesting.
fn is_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'[' || b == b']'
}

/// Map a field name to a [`Field`], honouring the `$key`/`$value` loop locals
/// inside a repetition.
fn push_field(fields: &mut FieldOptions, name: &str, rep_depth: usize) -> Result<(), FormatError> {
    if rep_depth > 0 {
        if name == "value" {
            fields.push(Field::ColValue);
            return Ok(());
        }
        if name == "key" {
            fields.push(Field::ColKey);
            return Ok(());
        }
        if let Some(rest) = name.strip_prefix("value.") {
            if let Field::Names(names) = parse_field(rest)? {
                fields.push(Field::ColValuePath(names));
                return Ok(());
            }
        }
    }
    crunch_field_options(name, fields)
}

fn crunch_field_options(content: &str, field_options: &mut FieldOptions) -> Result<(), FormatError> {
    if content.is_empty() {
        return Ok(());
    }
    for field in content.split('|') {
        if !field.is_empty() {
            field_options.push(parse_field(field)?);
        }
    }
    Ok(())
}

// parse a field str into list of possible names and/or index
// e.g. "field1.field2[0].field3" -> [Name("field1"), Name("field2"), Index(0),
// Name("field3")]
pub(super) fn parse_field(name: &str) -> Result<Field, FormatError> {
    // field is whole or rest
    if name == "." {
        return Ok(Field::Whole);
    } else if name == ".." {
        return Ok(Field::Rest);
    }

    let mut args = SmallVec::new();

    for part in name.split('.') {
        if let Some((name, index)) = part.split_once('[') {
            args.push(FieldType::Name(name.to_owned()));
            if index.ends_with(']') {
                let index = index
                    .trim_end_matches(']')
                    .parse()
                    .toss_parse_index_with(|| index.to_owned())?;
                args.push(FieldType::Index(index));
            } else {
                return Err(FormatError::IndexBracket);
            }
        } else {
            args.push(FieldType::Name(part.to_owned()));
        }
    }

    Ok(Field::Names(args))
}

pub fn parse_format(
    input: Option<&str>,
    no_color: bool,
    mut compact: bool,
) -> Result<Format, FormatError> {
    let mut style = (!no_color).then(Style::new);
    let mut is_json = false;
    let mut indent = 0;
    let mut is_level = false;
    let mut escape = Escape::None;
    let mut markup_styles = MarkupStyles::default();

    let Some(input) = input else {
        return Ok(Format {
            style,
            compact,
            is_json,
            indent,
            is_level,
            optional: false,
            escape,
            markup_styles,
        });
    };

    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let (name, value) = if let Some((name, value)) = part.split_once('=') {
            (name, value)
        } else {
            match part {
                // special type of modifier only applicable to level field,
                // where the style changes based on the level
                "level" => {
                    is_level = true;
                    continue;
                }
                "compact" => {
                    compact = true;
                    continue;
                }
                "json" => {
                    is_json = true;
                    continue;
                }
                // per-cell escapes for output formats
                "html" => {
                    escape = Escape::Html;
                    continue;
                }
                "csv" => {
                    escape = Escape::Csv;
                    continue;
                }
                "tsv" => {
                    escape = Escape::Tsv;
                    continue;
                }
                "md" => {
                    escape = Escape::Md;
                    continue;
                }
                "dimmed" => {
                    if let Some(s) = style.take() {
                        style = Some(s.dimmed());
                    }
                    continue;
                }
                "bold" => {
                    if let Some(s) = style.take() {
                        style = Some(s.bold());
                    }
                    continue;
                }
                _ => {}
            }

            ("fg", part)
        };

        match name {
            "fg" => {
                let color = parse_color(value).toss_parse_color()?;
                if let Some(s) = style.take() {
                    style = Some(s.color(color));
                }
            }
            "bg" => {
                let color = parse_color(value).toss_parse_color()?;
                if let Some(s) = style.take() {
                    style = Some(s.on_color(color));
                }
            }
            "indent" => {
                let Ok(value) = value.parse::<usize>() else {
                    return Err(FormatError::ParseIndent(value.to_owned()));
                };
                indent = value;
            }
            "key" => {
                let color = parse_color(value).toss_parse_color()?;
                markup_styles.key = markup_styles.key.color(color);
            }
            "value" => {
                let color = parse_color(value).toss_parse_color()?;
                markup_styles.value = markup_styles.value.color(color);
            }
            "str" => {
                let color = parse_color(value).toss_parse_color()?;
                markup_styles.str = markup_styles.str.color(color);
            }
            "syntax" => {
                let color = parse_color(value).toss_parse_color()?;
                markup_styles.syntax = markup_styles.syntax.color(color);
            }
            _ => return Err(FormatError::InvalidModifier(name.to_owned())),
        }
    }

    Ok(Format {
        style,
        compact,
        is_json,
        indent,
        is_level,
        optional: false,
        escape,
        markup_styles,
    })
}

use thiserror::Error;
use tosserror::Toss;

#[derive(Debug, Error, Toss)]
pub enum FormatError {
    #[error("Failed to parse color")]
    ParseColor { source: ParseColorError },
    #[error("Invalid indent value in format string '{0}'")]
    ParseIndent(String),
    #[error("Invalid modifier in format string '{0}'")]
    InvalidModifier(String),
    #[error("Unknown character escape in format string '\\{0}'")]
    UnknownCharEscape(char),
    #[error("Closing brace '}}' not found in format string")]
    ClosingBrace,
    #[error("Index closing bracket not found")]
    IndexBracket,
    #[error("Failed to parse index in format string '{value}'")]
    ParseIndex {
        source: ParseIntError,
        value: String,
    },
    #[error("Unsupported config value in formatter '{config}'")]
    UnsupportedConfig { config: String },
    #[error("A `$( … )` block is missing its closing `)` — check your `$(`, `$path(`, `$cols(`, `$rows(`, and `$name?(` blocks all have a matching `)`")]
    UnclosedRep,
    #[error("A '$( … )' repetition needs an operator ('*', '+', or '?') after it")]
    MissingRepOp,
    #[error("Unterminated quoted separator in a repetition")]
    UnterminatedSep,
}
