use std::num::ParseIntError;

use owo_colors::Style;
use smallvec::{smallvec, SmallVec};

use super::{Arg, Cond, Field, FieldType, Format, Formatter, Piece};
use crate::{
    colors::{parse_color, ParseColorError},
    json::MarkupStyles,
};

// Example log format
// '{#log}{#if spans|data}\n{spans|data:json}{/if}'
pub fn parse_formatter(
    input: &str,
    no_color: bool,
    compact: bool,
    variables: &[(String, String)],
) -> Result<Formatter, FormatError> {
    let mut pieces = Vec::new();
    let mut args = Vec::new();

    crunch_input(&mut pieces, &mut args, input, no_color, compact, variables)?;

    Ok(Formatter { pieces, args })
}

fn crunch_input(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    input: &str,
    no_color: bool,
    compact: bool,
    variables: &[(String, String)],
) -> Result<(), FormatError> {
    let mut chunks = input.split('\\');

    if let Some(chunk) = chunks.next() {
        crunch_chunk(pieces, args, chunk, no_color, compact, variables)?;
    }

    for chunk in chunks {
        let (escaped, rest) = chunk.split_at(1);
        pieces.push(parse_escaped(escaped.chars().next().unwrap())?);
        crunch_chunk(pieces, args, rest, no_color, compact, variables)?;
    }

    Ok(())
}

#[inline]
fn parse_escaped(c: char) -> Result<Piece, FormatError> {
    match c {
        'n' => Ok(Piece::Escaped('\n')),
        'r' => Ok(Piece::Escaped('\r')),
        't' => Ok(Piece::Escaped('\t')),
        '\'' => Ok(Piece::Escaped('\'')),
        '"' => Ok(Piece::Escaped('\"')),
        '{' => Ok(Piece::Escaped('{')),
        '}' => Ok(Piece::Escaped('}')),
        '\\' => Ok(Piece::Escaped('\\')),
        _ => Err(FormatError::UnknownCharEscape(c)),
    }
}

fn crunch_chunk(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    chunk: &str,
    no_color: bool,
    compact: bool,
    variables: &[(String, String)],
) -> Result<(), FormatError> {
    let mut parts = chunk.split('{');

    if let Some(part) = parts.next() {
        if !part.is_empty() {
            pieces.push(Piece::Literal(part.to_owned()));
        }
    }

    for part in parts {
        if let Some(end) = part.find('}') {
            let content = &part[..end];

            // '&' means a variable and needs to be expanded
            if let Some(key) = content.strip_prefix('&') {
                // if there's a formatting like `{&var:dimmed}`, crunch as field
                if key.contains(':') {
                    crunch_variable(pieces, args, key, no_color, compact, variables)?;
                } else {
                    let value = get_variable(variables, key)?;
                    crunch_input(pieces, args, value, no_color, compact, variables)?;
                }
            } else if let Some(content) = content.strip_prefix('#') {
                // '#' means param is a conditional
                crunch_cond(pieces, args, content, no_color, compact, variables)?;
            } else if let Some(content) = content.strip_prefix(':') {
                // ':' means `else` of conditional
                crunch_cond_else(pieces, args, content, no_color, compact, variables)?;
            } else if let Some(content) = content.strip_prefix('/') {
                // '/' means end of conditional
                crunch_cond_end(pieces, content)?;
            } else {
                crunch_field(pieces, args, content, no_color, compact)?;
            }

            let literal = &part[end + 1..];
            if !literal.is_empty() {
                pieces.push(Piece::Literal(literal.to_owned()));
            }
        } else {
            return Err(FormatError::ClosingBrace);
        }
    }

    Ok(())
}

#[inline]
fn crunch_cond(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    content: &str,
    no_color: bool,
    compact: bool,
    variables: &[(String, String)],
) -> Result<(), FormatError> {
    let (content, cond) = if let Some(content) = content.strip_prefix("if ") {
        (content, Cond::If)
    } else if let Some(content) = content.strip_prefix("key ") {
        (content, Cond::Key)
    } else {
        return Err(FormatError::UnsupportedFunction {
            func: content.to_owned(),
        });
    };

    let content = content
        .strip_prefix('&')
        .map(|k| get_variable(variables, k))
        .transpose()?
        .unwrap_or(content);

    let arg = parse_arg(content, no_color, compact)?;
    args.push(arg);
    pieces.push(Piece::CondStart(cond, args.len() - 1));

    Ok(())
}

#[inline]
fn crunch_cond_else(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    content: &str,
    no_color: bool,
    compact: bool,
    variables: &[(String, String)],
) -> Result<(), FormatError> {
    let (content, cond) = if let Some(content) = content.strip_prefix("else if ") {
        (content, Cond::If)
    } else if let Some(content) = content.strip_prefix("else key ") {
        (content, Cond::Key)
    } else if content == "else" {
        pieces.push(Piece::Else);
        return Ok(());
    } else {
        return Err(FormatError::UnsupportedFunction {
            func: content.to_owned(),
        });
    };

    let content = content
        .strip_prefix('&')
        .map(|k| get_variable(variables, k))
        .transpose()?
        .unwrap_or(content);

    let arg = parse_arg(content, no_color, compact)?;
    args.push(arg);
    pieces.push(Piece::ElseCond(cond, args.len() - 1));

    Ok(())
}

#[inline]
fn crunch_cond_end(pieces: &mut Vec<Piece>, content: &str) -> Result<(), FormatError> {
    if content == "if" {
        pieces.push(Piece::CondEnd);
    }

    Ok(())
}

#[inline]
fn crunch_field(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    content: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    let arg = parse_arg(content, no_color, compact)?;

    args.push(arg);
    pieces.push(Piece::Arg(args.len() - 1));

    Ok(())
}

fn parse_arg(content: &str, no_color: bool, compact: bool) -> Result<Arg, FormatError> {
    let content = content.trim();

    // param is a field
    let (name_part, format) = match content.split_once(':') {
        Some((name, styles)) => (name, parse_format(Some(styles), no_color, compact)?),
        None => (content, parse_format(None, no_color, compact)?),
    };

    let mut names = SmallVec::new();

    // if "", then print the whole json
    if name_part.is_empty() {
        names.push(smallvec![FieldType::Name("".to_owned())]);
    } else {
        for name in name_part.split('|') {
            names.push(parse_name(name)?);
        }
    }

    Ok((names, format))
}

#[inline]
fn crunch_variable(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    key: &str,
    no_color: bool,
    compact: bool,
    variables: &[(String, String)],
) -> Result<(), FormatError> {
    let key = key.trim();

    // if there's a formatting like `{&var:dimmed}`, crunch as field
    if let Some((key, styles)) = key.split_once(':') {
        let format = parse_format(Some(styles), no_color, compact)?;
        let name_part = get_variable(variables, key)?;

        let mut names = SmallVec::new();

        // if "", then print the whole json
        if name_part.is_empty() {
            names.push(smallvec![FieldType::Name("".to_owned())]);
        } else {
            for name in name_part.split('|') {
                names.push(parse_name(name)?);
            }
        }

        args.push((names, format));
        pieces.push(Piece::Arg(args.len() - 1));
    } else {
        let value = get_variable(variables, key)?;
        crunch_input(pieces, args, value, no_color, compact, variables)?;
    }

    Ok(())
}

// parse a name str into list of possible names and/or index
// e.g. "field1.field2[0].field3" -> [Name("field1"), Name("field2"), Index(0),
// Name("field3")]
fn parse_name(name: &str) -> Result<Field, FormatError> {
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

    Ok(args)
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
    let mut markup_styles = MarkupStyles::default();

    let Some(input) = input else {
        return Ok(Format {
            style,
            compact,
            is_json,
            indent,
            is_level,
            markup_styles,
        });
    };

    for part in input.split(',') {
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
        markup_styles,
    })
}

#[inline]
fn get_variable<'a>(variables: &'a [(String, String)], key: &str) -> Result<&'a str, FormatError> {
    variables
        .iter()
        .find_map(|(k, v)| (k == key).then_some(v.as_str()))
        .ok_or_else(|| FormatError::InvalidVariable {
            variable: key.to_owned(),
        })
}

use thiserror::Error;
use tosserror::Toss;

#[derive(Debug, Error, Toss)]
pub enum FormatError {
    #[error("Failed to parse color")]
    ParseColor {
        source: ParseColorError,
    },
    #[error("Invalid indent value in format string '{0}'")]
    ParseIndent(String),
    #[error("Invalid modifier in format string '{0}'")]
    InvalidModifier(String),
    #[error("Unknown character escape in format string '\\{0}'")]
    UnknownCharEscape(char),
    #[error("Closing brace not found in format string")]
    ClosingBrace,
    #[error("Index closing bracket not found")]
    IndexBracket,
    #[error("Failed to parse index in format string '{value}'")]
    ParseIndex {
        source: ParseIntError,
        value: String,
    },
    #[error("Unsupported function '{func}'")]
    UnsupportedFunction { func: String },
    #[error("Variable doesn't exist: {variable}")]
    InvalidVariable { variable: String },
}
