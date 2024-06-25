use super::{Arg, Field, FieldType, Format, Formatter, Piece};
use crate::{
    colors::{parse_color, ParseColorError},
    json::MarkupStyles,
};
use owo_colors::Style;
use smallvec::{smallvec, SmallVec};
use std::num::ParseIntError;

// Example log format
// '{#log}{#if spans|data}\n{spans|data:json}{/if}'
pub fn parse_formatter(
    input: &str,
    no_color: bool,
    compact: bool,
) -> Result<Formatter, FormatError> {
    let mut pieces = Vec::new();
    let mut args = Vec::new();

    let mut chunks = input.split('\\');

    if let Some(chunk) = chunks.next() {
        parse_chunk(&mut pieces, &mut args, chunk, no_color, compact)?;
    }

    for chunk in chunks {
        let (escaped, rest) = chunk.split_at(1);
        pieces.push(parse_escaped(escaped.chars().next().unwrap())?);
        parse_chunk(&mut pieces, &mut args, rest, no_color, compact)?;
    }

    Ok(Formatter { pieces, args })
}

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

fn parse_chunk(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    chunk: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    let mut parts = chunk.split('{');

    if let Some(part) = parts.next() {
        if !part.is_empty() {
            pieces.push(Piece::Literal(part.to_owned()));
        }
    }

    for part in parts {
        if let Some(end) = part.find('}') {
            let param = &part[..end];

            // '#' means param is a function
            if let Some(content) = param.strip_prefix('#') {
                parse_func(pieces, args, content, no_color, compact)?;
            } else if let Some(content) = param.strip_prefix('/') {
                // '/' means end of function
                parse_func_end(pieces, content)?;
            } else {
                parse_field(pieces, args, param, no_color, compact)?;
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

fn parse_func(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    content: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    if content == "log" {
        parse_func_log(pieces, args, no_color, compact)?;
    } else if let Some(content) = content.strip_prefix("if ") {
        let arg = parse_arg(content, no_color, compact)?;
        args.push(arg);
        pieces.push(Piece::IfStart(args.len() - 1));
    } else {
        return Err(FormatError::UnsupportedFunction {
            func: content.to_owned(),
        });
    }

    Ok(())
}

fn parse_func_log(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    // default log format
    let timestamp = smallvec![smallvec![FieldType::Name("timestamp".to_owned())]];
    let timestamp_fmt = parse_format(Some("dimmed"), no_color, compact)?;
    args.push((timestamp, timestamp_fmt));
    pieces.push(Piece::Arg(args.len() - 1));
    pieces.push(Piece::Literal(" ".to_owned()));

    let level = smallvec![
        smallvec![FieldType::Name("level".to_owned())],
        smallvec![FieldType::Name("lvl".to_owned())],
        smallvec![FieldType::Name("severity".to_owned())],
    ];
    let level_fmt = parse_format(Some("level"), no_color, compact)?;
    args.push((level, level_fmt));
    pieces.push(Piece::Arg(args.len() - 1));
    pieces.push(Piece::Literal(" ".to_owned()));

    let message = smallvec![
        smallvec![FieldType::Name("message".to_owned())],
        smallvec![FieldType::Name("msg".to_owned())],
        smallvec![FieldType::Name("body".to_owned())],
    ];
    let message_fmt = parse_format(None, no_color, compact)?;
    args.push((message, message_fmt));
    pieces.push(Piece::Arg(args.len() - 1));

    Ok(())
}

fn parse_func_end(pieces: &mut Vec<Piece>, content: &str) -> Result<(), FormatError> {
    if content == "if" {
        pieces.push(Piece::IfEnd);
    }

    Ok(())
}

fn parse_field(
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
            if !name.is_empty() {
                names.push(parse_name(name)?);
            }
        }
    }

    Ok((names, format))
}

// parse a name str into list of possible names and/or index
// e.g. "field1.field2[0].field3" -> [Name("field1"), Name("field2"), Index(0), Name("field3")]
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
                // special type of modifier only applicable to level field, where the style changes based on the level
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
}
