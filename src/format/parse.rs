use std::num::ParseIntError;

use owo_colors::Style;
use smallvec::SmallVec;

use super::{Arg, Cond, Field, FieldOptions, FieldType, Format, Piece};
use crate::{
    colors::{parse_color, ParseColorError},
    json::MarkupStyles,
};

pub(super) fn crunch_input(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    input: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    let mut chunks = input.split('\\');

    if let Some(chunk) = chunks.next() {
        crunch_chunk(pieces, args, chunk, no_color, compact)?;
    }

    let mut prev_was_backslash = false;

    for chunk in chunks {
        if !chunk.is_empty() {
            if prev_was_backslash {
                crunch_chunk(pieces, args, chunk, no_color, compact)?;
            } else {
                let (escaped, rest) = chunk.split_at(1);
                pieces.push(parse_escaped(escaped.chars().next().unwrap())?);
                if !rest.is_empty() {
                    crunch_chunk(pieces, args, rest, no_color, compact)?;
                }
            }
            prev_was_backslash = false;
        } else {
            pieces.push(parse_escaped('\\')?);
            prev_was_backslash = true;
        }
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

            // '#' means param is a conditional
            if let Some(content) = content.strip_prefix('#') {
                crunch_cond(pieces, args, content, no_color, compact)?;
            } else if let Some(content) = content.strip_prefix(':') {
                // ':' means `else` of conditional
                crunch_cond_else(pieces, args, content, no_color, compact)?;
            } else if content.starts_with('/') {
                // '/' means end of conditional
                crunch_cond_end(pieces)?;
            } else {
                crunch_arg(pieces, args, content, no_color, compact)?;
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
) -> Result<(), FormatError> {
    let (content, cond) = if let Some(content) = content.strip_prefix("if ") {
        (content, Cond::If)
    } else if let Some(content) = content.strip_prefix("key ") {
        (content, Cond::Key)
    } else if let Some(content) = content.strip_prefix("config ") {
        let b = if content == "compact" {
            compact
        } else if content == "no_color" {
            no_color
        } else {
            return Err(FormatError::UnsupportedConfig {
                config: content.to_owned(),
            });
        };
        pieces.push(Piece::CondStart(Cond::IfConfig(b), 0));

        return Ok(());
    } else {
        return Err(FormatError::UnsupportedConditional {
            cond: format!("#{content}"),
        });
    };

    let mut field_options = FieldOptions::new();
    crunch_field_options(content, &mut field_options)?;

    args.push((field_options, parse_format(None, no_color, compact)?));
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
) -> Result<(), FormatError> {
    let (content, cond) = if let Some(content) = content.strip_prefix("else if ") {
        (content, Cond::If)
    } else if let Some(content) = content.strip_prefix("else key ") {
        (content, Cond::Key)
    } else if content == "else" {
        pieces.push(Piece::Else);
        return Ok(());
    } else {
        return Err(FormatError::UnsupportedConditional {
            cond: format!(":{content}"),
        });
    };

    let mut field_options = FieldOptions::new();
    crunch_field_options(content, &mut field_options)?;

    args.push((field_options, parse_format(None, no_color, compact)?));
    pieces.push(Piece::ElseCond(cond, args.len() - 1));

    Ok(())
}

#[inline]
fn crunch_cond_end(pieces: &mut Vec<Piece>) -> Result<(), FormatError> {
    pieces.push(Piece::CondEnd);

    Ok(())
}

#[inline]
fn crunch_arg(
    pieces: &mut Vec<Piece>,
    args: &mut Vec<Arg>,
    content: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    let content = content.trim();

    // param is a field
    let (name_part, format) = match content.split_once(':') {
        Some((name, styles)) => (name, parse_format(Some(styles), no_color, compact)?),
        None => (content, parse_format(None, no_color, compact)?),
    };

    let mut fields = FieldOptions::new();
    crunch_field_options(name_part, &mut fields)?;

    args.push((fields, format));
    pieces.push(Piece::Arg(args.len() - 1));

    Ok(())
}

fn crunch_field_options(
    content: &str,
    field_options: &mut FieldOptions,
) -> Result<(), FormatError> {
    if content.is_empty() {
        return Ok(());
    } else {
        for field in content.split('|') {
            if !field.is_empty() {
                field_options.push(parse_field(field)?);
            }
        }
    }

    Ok(())
}

// parse a field str into list of possible names and/or index
// e.g. "field1.field2[0].field3" -> [Name("field1"), Name("field2"), Index(0),
// Name("field3")]
fn parse_field(name: &str) -> Result<Field, FormatError> {
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
    #[error("Unsupported conitional '{cond}'")]
    UnsupportedConditional { cond: String },
    #[error("Unsupported config value in formatter '{config}'")]
    UnsupportedConfig { config: String },
}
