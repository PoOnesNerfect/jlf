use std::num::ParseIntError;

use owo_colors::Style;
use smallvec::SmallVec;

use super::{
    Arg, ArmTest, CmpOp, Cond, Escape, Field, FieldOptions, FieldType, Format,
    Piece, RepOp, RepSource,
};
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
    sc.scan(pieces, args, 0, Stop::Eof)?;
    absorb_line_prefixes(pieces);
    Ok(())
}

/// When a `$( … )` / `$path( … )` block starts its own line in the
/// template, pull the preceding newline + indentation into the front of the
/// block's body. That way each block can be written on its own indented source
/// line for readability, while the line break renders per iteration (reps) or
/// once (conditionals) — not unconditionally before the block.
fn absorb_line_prefixes(pieces: &mut Vec<Piece>) {
    let mut i = 0;
    while i < pieces.len() {
        let is_block =
            matches!(pieces[i], Piece::RepStart(..) | Piece::CondStart(..));
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

/// Byte index where a trailing `\n[ \t]*` run begins (block sits at a line
/// start), or `None` if the literal doesn't end that way.
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

/// Where a `scan` pass stops.
#[derive(Clone, Copy, PartialEq)]
enum Stop {
    /// Top level: scan to the end of input.
    Eof,
    /// Inside `$( … )`: stop just past the matching, paren-balanced `)`.
    Rep,
}

/// Which `$if`/`$has`/`$config` opener a condition chain starts with.
#[derive(Clone, Copy)]
enum CondKind {
    If,
    Has,
    Config,
}

impl Scanner<'_> {
    /// Scan into `pieces` until `stop` is reached.
    fn scan(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
        stop: Stop,
    ) -> Result<(), FormatError> {
        let in_rep = stop == Stop::Rep;
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
                        self.parse_rep(
                            pieces,
                            args,
                            rep_depth,
                            RepSource::Record,
                        )?;
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
                        if c == b'\\'
                            || c == b'$'
                            || (in_rep && (c == b'(' || c == b')'))
                        {
                            break;
                        }
                        self.i += 1;
                    }
                    lit.push_str(
                        std::str::from_utf8(&self.b[start..self.i]).unwrap(),
                    );
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
                "match" => return self.parse_match(pieces, args, rep_depth),
                "if" => {
                    return self.parse_cond_chain(
                        pieces,
                        args,
                        rep_depth,
                        CondKind::If,
                    )
                }
                "has" => {
                    return self.parse_cond_chain(
                        pieces,
                        args,
                        rep_depth,
                        CondKind::Has,
                    )
                }
                "config" => {
                    return self.parse_cond_chain(
                        pieces,
                        args,
                        rep_depth,
                        CondKind::Config,
                    )
                }
                // `$elif`/`$else`/`$when` only appear as chain/arm
                // continuations, parsed by their opener;
                // standalone they are an error.
                "elif" | "else" | "when" => {
                    return Err(FormatError::OrphanArm(name.to_owned()))
                }
                "cols" => RepSource::Columns,
                _ => RepSource::Path(parse_field(name)?),
            };
            return self.parse_rep(pieces, args, rep_depth, src);
        }
        let mut fields = FieldOptions::new();
        push_field(&mut fields, name, rep_depth)?;
        args.push((fields, parse_format(None, self.no_color, self.compact)?));
        pieces.push(Piece::Arg(args.len() - 1));
        Ok(())
    }

    /// Parse `$match( subject  $when(pat => body) … $else(body) )`. The subject
    /// (first token, fallbacks allowed) is bound to `$value` for every arm
    /// body, and the first arm whose pattern matches renders. A missing
    /// subject matches no arm — not even `$else` — so it renders nothing.
    /// Cursor is just past `$match(`.
    fn parse_match(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
    ) -> Result<(), FormatError> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.b.len()
            && (is_path_byte(self.b[self.i]) || self.b[self.i] == b'|')
        {
            self.i += 1;
        }
        if self.i == start {
            return Err(FormatError::EmptyMatchSubject);
        }
        let subject = std::str::from_utf8(&self.b[start..self.i]).unwrap();
        let mut fo = FieldOptions::new();
        push_field(&mut fo, subject, rep_depth)?;
        args.push((fo, parse_format(None, self.no_color, self.compact)?));
        pieces.push(Piece::MatchStart(args.len() - 1));

        let mut first = true;
        loop {
            self.skip_ws();
            match self.b.get(self.i) {
                Some(b')') => {
                    self.i += 1;
                    break;
                }
                Some(b'$') if self.rest_is("$when(") => {
                    self.i += 6;
                    let pat = self.read_until_arrow()?;
                    self.push_arm(
                        pieces,
                        args,
                        rep_depth,
                        parse_arm(&pat)?,
                        first,
                    )?;
                }
                Some(b'$') if self.rest_is("$else(") => {
                    self.i += 6;
                    self.skip_hspace();
                    self.push_arm(
                        pieces,
                        args,
                        rep_depth,
                        Cond::Arm(Vec::new()),
                        first,
                    )?;
                }
                _ => return Err(FormatError::BadMatchArm),
            }
            first = false;
        }
        pieces.push(Piece::CondEnd);
        pieces.push(Piece::MatchEnd);
        Ok(())
    }

    /// Emit one match arm: the arm condition tests the bound subject
    /// (`$value`), then the body is scanned up to the arm's closing `)`.
    fn push_arm(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
        cond: Cond,
        first: bool,
    ) -> Result<(), FormatError> {
        let mut fo = FieldOptions::new();
        fo.push(Field::ColValue);
        args.push((fo, parse_format(None, self.no_color, self.compact)?));
        let idx = args.len() - 1;
        pieces.push(if first {
            Piece::CondStart(cond, idx)
        } else {
            Piece::ElseCond(cond, idx)
        });
        let mut body = Vec::new();
        self.scan(&mut body, args, rep_depth + 1, Stop::Rep)?;
        trim_cond_body(&mut body);
        pieces.extend(body);
        Ok(())
    }

    /// True when the input at the cursor starts with `s`.
    fn rest_is(&self, s: &str) -> bool {
        self.b[self.i..].starts_with(s.as_bytes())
    }

    /// Read a `$when`/`$if`/… condition up to its `=>` (outside quotes),
    /// consuming the `=>` and one following space/tab (the syntactic separator
    /// before the body). Returns the condition text trimmed.
    fn read_until_arrow(&mut self) -> Result<String, FormatError> {
        let start = self.i;
        let mut quote = 0u8;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            match c {
                q @ (b'"' | b'\'') if quote == 0 => quote = q,
                q if quote == q => quote = 0,
                b'=' if quote == 0 && self.b.get(self.i + 1) == Some(&b'>') => {
                    let pat = std::str::from_utf8(&self.b[start..self.i])
                        .unwrap()
                        .trim()
                        .to_owned();
                    self.i += 2; // consume `=>`
                    self.skip_hspace();
                    return Ok(pat);
                }
                b')' if quote == 0 => break,
                _ => {}
            }
            self.i += 1;
        }
        Err(FormatError::MissingArrow)
    }

    /// Skip a single space or tab (the syntactic separator after `=>` / `(`),
    /// leaving any deliberate extra spaces in the body.
    fn skip_hspace(&mut self) {
        if matches!(self.b.get(self.i), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }

    /// Skip spaces, tabs, and newlines (decorative layout between
    /// arms/branches).
    fn skip_ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.i += 1;
        }
    }

    /// Parse an `$if`/`$has`/`$config` condition chain: the opener plus any
    /// adjacent `$elif( … )` / `$else( … )` continuations (whitespace between
    /// them is ignored). Emits `CondStart` + `ElseCond*` + optional `Else` +
    /// `CondEnd`, the same shape the renderer walks. Cursor is just past the
    /// opener's `(`.
    fn parse_cond_chain(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
        kind: CondKind,
    ) -> Result<(), FormatError> {
        self.push_cond_open(pieces, args, rep_depth, kind, false)?;
        loop {
            let save = self.i;
            self.skip_ws();
            if self.rest_is("$elif(") {
                self.i += 6;
                self.push_cond_open(
                    pieces,
                    args,
                    rep_depth,
                    CondKind::If,
                    true,
                )?;
            } else if self.rest_is("$else(") {
                self.i += 6;
                self.skip_hspace();
                pieces.push(Piece::Else);
                let mut body = Vec::new();
                self.scan(&mut body, args, rep_depth, Stop::Rep)?;
                trim_cond_body(&mut body);
                pieces.extend(body);
                break;
            } else {
                self.i = save; // no continuation — leave following text intact
                break;
            }
        }
        pieces.push(Piece::CondEnd);
        Ok(())
    }

    /// Emit one `$if`/`$elif`/`$has`/`$config` branch: read `COND =>`, push the
    /// condition marker, then scan the body up to the branch's `)`.
    fn push_cond_open(
        &mut self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        rep_depth: usize,
        kind: CondKind,
        is_else: bool,
    ) -> Result<(), FormatError> {
        let cond_text = self.read_until_arrow()?;
        match kind {
            CondKind::If => {
                self.push_cond(pieces, args, &cond_text, Cond::If, is_else)?
            }
            CondKind::Has => {
                self.push_cond(pieces, args, &cond_text, Cond::Has, is_else)?
            }
            CondKind::Config => {
                let b = match cond_text.trim() {
                    "compact" => self.compact,
                    "no_color" => self.no_color,
                    other => {
                        return Err(FormatError::UnsupportedConfig {
                            config: other.to_owned(),
                        })
                    }
                };
                pieces.push(if is_else {
                    Piece::ElseCond(Cond::IfConfig(b), 0)
                } else {
                    Piece::CondStart(Cond::IfConfig(b), 0)
                });
            }
        }
        let mut body = Vec::new();
        self.scan(&mut body, args, rep_depth, Stop::Rep)?;
        trim_cond_body(&mut body);
        pieces.extend(body);
        Ok(())
    }

    /// Parse a `${ … }` group: an interpolated field. (Conditionals are the
    /// `$if( … )` / `$has( … )` / `$config( … )` block forms, not brace
    /// directives.)
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
        let content =
            std::str::from_utf8(&self.b[start..self.i]).unwrap().trim();
        self.i += 1; // consume '}'
        self.push_arg(pieces, args, content, rep_depth)
    }

    fn push_cond(
        &self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        content: &str,
        cond: Cond,
        is_else: bool,
    ) -> Result<(), FormatError> {
        // Only `if`/`else if` support value comparisons (`field OP literal`);
        // `key` just tests presence, so its content is always a field path.
        let (field_str, cond) = match cond {
            Cond::If => match split_comparison(content) {
                Some((lhs, op, rhs)) => {
                    (lhs, Cond::Cmp(op, unquote(rhs).to_owned()))
                }
                None => (content.trim(), Cond::If),
            },
            other => (content.trim(), other),
        };
        let mut fo = FieldOptions::new();
        crunch_field_options(field_str, &mut fo)?;
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
        // A quoted hole is a literal, not a field: `${"text":mods}` renders the
        // text with the arg's modifiers (so raw text can be styled/escaped).
        if let Some(quote) = content.trim_start().chars().next() {
            if quote == '"' || quote == '\'' {
                return self.push_literal_arg(
                    pieces,
                    args,
                    content.trim_start(),
                    quote,
                );
            }
        }
        let (name_part, mut format) = match content.split_once(':') {
            Some((n, styles)) => (
                n.trim(),
                parse_format(Some(styles), self.no_color, self.compact)?,
            ),
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

    /// Parse a `${"text":mods}` quoted-literal hole: read the quoted text (with
    /// `\n`/`\t`/`\r`/`\\`/`\"`/`\'` escapes), then apply any `:modifiers`.
    fn push_literal_arg(
        &self,
        pieces: &mut Vec<Piece>,
        args: &mut Vec<Arg>,
        content: &str,
        quote: char,
    ) -> Result<(), FormatError> {
        let mut lit = String::new();
        let mut chars = content.char_indices();
        chars.next(); // consume the opening quote
        let mut escaped = false;
        let mut close_end = None;
        for (idx, c) in chars {
            if escaped {
                match c {
                    'n' => lit.push('\n'),
                    't' => lit.push('\t'),
                    'r' => lit.push('\r'),
                    other => lit.push(other), /* \\, \", \', and any other:
                                               * literal */
                }
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == quote {
                close_end = Some(idx + c.len_utf8());
                break;
            } else {
                lit.push(c);
            }
        }
        let close_end = close_end.ok_or(FormatError::UnterminatedLiteral)?;
        let styles = match content[close_end..].trim() {
            "" => None,
            rest => Some(rest.strip_prefix(':').ok_or_else(|| {
                FormatError::LiteralTrailing(rest.to_owned())
            })?),
        };
        let format = parse_format(styles, self.no_color, self.compact)?;
        let mut fields = FieldOptions::new();
        fields.push(Field::Literal(lit));
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
        self.scan(&mut body, args, rep_depth + 1, Stop::Rep)?;
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
                String::from_utf8(bytes)
                    .map_err(|_| FormatError::UnterminatedSep)
            }
            Some(_) => {
                // A bare (unquoted) separator is a short run right after `)`,
                // e.g. `,` in `$cols( $key ),*`. Stop at a newline or a
                // template marker so a malformed repetition (an
                // operator-less `$x( … )`) can't silently
                // swallow the following line/blocks as its
                // "separator" — it surfaces as a clear MissingRepOp error.
                let start = self.i;
                while let Some(&c) = self.b.get(self.i) {
                    if matches!(
                        c,
                        b'*' | b'+' | b'?' | b'\n' | b'\r' | b'$' | b'{'
                    ) {
                        break;
                    }
                    self.i += 1;
                }
                Ok(std::str::from_utf8(&self.b[start..self.i])
                    .unwrap()
                    .to_owned())
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

/// Trim a `$if`/`$when`/`$else`/… body. An all-whitespace body is an
/// intentional separator (e.g. `$else(\n)`, `$config(compact =>  )`) and is
/// kept as-is. Otherwise, drop leading/trailing whitespace runs that span a
/// newline (decorative line breaks + indentation), so an arm can be written
/// across lines while its output stays on one line; same-line padding is
/// preserved.
fn trim_cond_body(body: &mut [Piece]) {
    let all_ws = body
        .iter()
        .all(|p| matches!(p, Piece::Literal(s) if s.chars().all(char::is_whitespace)));
    if all_ws {
        return;
    }
    if let Some(Piece::Literal(s)) = body.first_mut() {
        if let Some(cut) = leading_layout_ws(s) {
            *s = s[cut..].to_owned();
        }
    }
    if let Some(Piece::Literal(s)) = body.last_mut() {
        if let Some(cut) = trailing_layout_ws(s) {
            s.truncate(cut);
        }
    }
}

/// End of a leading whitespace run when it spans a newline, else `None`.
fn leading_layout_ws(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    s[..i].contains('\n').then_some(i)
}

/// Start of a trailing whitespace run when it spans a newline, else `None`.
fn trailing_layout_ws(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = b.len();
    while i > 0 && matches!(b[i - 1], b' ' | b'\t' | b'\r' | b'\n') {
        i -= 1;
    }
    s[i..].contains('\n').then_some(i)
}

fn is_ident_start(b: u8) -> bool { b.is_ascii_alphabetic() || b == b'_' }

/// Bytes allowed in a bare field path: identifiers plus `.`/`[`/`]` for
/// nesting.
fn is_path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || b == b'_'
        || b == b'.'
        || b == b'['
        || b == b']'
}

/// Split a comparison condition (`field OP literal`) into its parts, or `None`
/// if no operator is present. Two-character operators are tried before their
/// single-character prefixes so `>=` isn't mistaken for `>`.
fn split_comparison(content: &str) -> Option<(&str, CmpOp, &str)> {
    for (token, op) in CMP_OPS {
        if let Some(idx) = content.find(token) {
            let lhs = content[..idx].trim();
            let rhs = content[idx + token.len()..].trim();
            return Some((lhs, op, rhs));
        }
    }
    None
}

/// Strip one pair of matching single/double quotes from a comparison literal.
fn unquote(s: &str) -> &str {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        if (first == b'"' || first == b'\'') && bytes[bytes.len() - 1] == first
        {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// The comparison operators, longest-first so `>=` isn't read as `>`.
const CMP_OPS: [(&str, CmpOp); 6] = [
    ("==", CmpOp::Eq),
    ("!=", CmpOp::Ne),
    (">=", CmpOp::Ge),
    ("<=", CmpOp::Le),
    (">", CmpOp::Gt),
    ("<", CmpOp::Lt),
];

/// Parse a `$when(pattern => …)` pattern into its arm condition. `$else` uses
/// an empty test list (the wildcard); otherwise it's `test | test | …`
/// alternation.
fn parse_arm(content: &str) -> Result<Cond, FormatError> {
    let c = content.trim();
    let mut tests = Vec::new();
    for alt in split_top_level_pipe(c) {
        tests.push(parse_arm_test(alt.trim())?);
    }
    Ok(Cond::Arm(tests))
}

/// One alternative of an arm pattern: a comparison (`>= 500`), a range
/// (`400..500`), or a bare literal (`200`, `"GET"`) treated as equality.
fn parse_arm_test(s: &str) -> Result<ArmTest, FormatError> {
    for (token, op) in CMP_OPS {
        if let Some(rest) = s.strip_prefix(token) {
            return Ok(ArmTest::Cmp(op, unquote(rest.trim()).to_owned()));
        }
    }
    if s.contains("..") {
        return parse_range(s);
    }
    Ok(ArmTest::Cmp(CmpOp::Eq, unquote(s).to_owned()))
}

/// Parse a numeric range arm: `lo..hi`, `lo..=hi`, `lo..`, `..hi`, `..=hi`.
fn parse_range(s: &str) -> Result<ArmTest, FormatError> {
    let (lo_str, rest) = s.split_once("..").unwrap();
    let hi_inclusive = rest.starts_with('=');
    let hi_str = if hi_inclusive { &rest[1..] } else { rest };
    let parse_bound = |b: &str| -> Result<Option<f64>, FormatError> {
        let b = b.trim();
        if b.is_empty() {
            return Ok(None);
        }
        b.parse::<f64>()
            .map(Some)
            .map_err(|_| FormatError::BadRange(s.to_owned()))
    };
    Ok(ArmTest::Range {
        lo: parse_bound(lo_str)?,
        hi: parse_bound(hi_str)?,
        hi_inclusive,
    })
}

/// Split on `|` that sits outside quotes (arm-pattern alternation).
fn split_top_level_pipe(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let b = s.as_bytes();
    let (mut start, mut quote) = (0usize, 0u8);
    for i in 0..b.len() {
        match b[i] {
            q @ (b'"' | b'\'') if quote == 0 => quote = q,
            q if quote == q => quote = 0,
            b'|' if quote == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Map a field name to a [`Field`], honouring the `$key`/`$value` loop locals
/// inside a repetition.
fn push_field(
    fields: &mut FieldOptions,
    name: &str,
    rep_depth: usize,
) -> Result<(), FormatError> {
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

fn crunch_field_options(
    content: &str,
    field_options: &mut FieldOptions,
) -> Result<(), FormatError> {
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
    let mut escape = Escape::None;
    let mut markup_styles = MarkupStyles::default();

    let Some(input) = input else {
        return Ok(Format {
            style,
            compact,
            is_json,
            indent,
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
                let color = parse_color(value)
                    .toss_parse_color_with(|| value.to_owned())?;
                if let Some(s) = style.take() {
                    style = Some(s.color(color));
                }
            }
            "bg" => {
                let color = parse_color(value)
                    .toss_parse_color_with(|| value.to_owned())?;
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
                let color = parse_color(value)
                    .toss_parse_color_with(|| value.to_owned())?;
                markup_styles.key = markup_styles.key.color(color);
            }
            "value" => {
                let color = parse_color(value)
                    .toss_parse_color_with(|| value.to_owned())?;
                markup_styles.value = markup_styles.value.color(color);
            }
            "str" => {
                let color = parse_color(value)
                    .toss_parse_color_with(|| value.to_owned())?;
                markup_styles.str = markup_styles.str.color(color);
            }
            "syntax" => {
                let color = parse_color(value)
                    .toss_parse_color_with(|| value.to_owned())?;
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
        optional: false,
        escape,
        markup_styles,
    })
}

use thiserror::Error;
use tosserror::Toss;

#[derive(Debug, Error, Toss)]
pub enum FormatError {
    #[error(
        "Failed to parse color '{value}' — if this was a style modifier, it \
         is not one jlf knows (note: the `:level` modifier was removed; use \
         the `${{@level}}` recipe or a `$match` to color by level)"
    )]
    ParseColor {
        source: ParseColorError,
        value: String,
    },
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
    #[error(
        "A `$( … )` block is missing its closing `)` — check your `$(`, \
         `$path(`, `$cols(`, and `$rows(` blocks all have a matching `)`"
    )]
    UnclosedRep,
    #[error(
        "A `$( … )` repetition needs an operator (`*`, `+`, or `?`) after it \
         — if you meant a conditional, use `$has(path => …)`, `$if(cond => \
         …)`, or `$match(subject $when(…))` instead (note: `$key`/`$value` \
         are loop variables, not conditionals)"
    )]
    MissingRepOp,
    #[error("Unterminated quoted separator in a repetition")]
    UnterminatedSep,
    #[error("A `$match( … )` needs a subject, e.g. `$match(status $when(…))`")]
    EmptyMatchSubject,
    #[error(
        "A `$match( … )` arm must be `$when(pattern => body)` or `$else(body)`"
    )]
    BadMatchArm,
    #[error(
        "A `$when`/`$if`/`$elif`/`$has`/`$config` block is missing its `=>` \
         between the condition and the body"
    )]
    MissingArrow,
    #[error(
        "`$when`/`$elif`/`$else` may only appear inside `$match( … )` or an \
         `$if`/`$has`/`$config` chain, not on their own: `${0}( … )`"
    )]
    OrphanArm(String),
    #[error("Invalid numeric range in a `$when( … )` arm: '{0}'")]
    BadRange(String),

    #[error(
        "Unterminated quoted literal in `${{ … }}` — a `${{\"text\"}}` \
         literal needs a closing quote"
    )]
    UnterminatedLiteral,

    #[error(
        "Unexpected text after a `${{\"text\"}}` literal — only `:modifiers` \
         may follow (got '{0}')"
    )]
    LiteralTrailing(String),
}
