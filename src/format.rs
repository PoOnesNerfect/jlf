use core::fmt;
use owo_colors::AnsiColors;
use smallvec::{smallvec, SmallVec};
use std::num::ParseIntError;

use crate::{
    colors::{parse_color, ParseColorError},
    json::MarkupStyles,
    Json,
};

pub use owo_colors::{OwoColorize as Colorize, Style};

// Example log format
// '{#log}{#for span in spans} {span.name}{/for}'
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
    args: &mut Args,
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
    args: &mut Args,
    content: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    let (func, _rest) = content
        .split_once(' ')
        .map(|(f, r)| (f, Some(r)))
        .unwrap_or((&content, None));

    match func {
        "log" => {
            // default log format
            let timestamp = smallvec![smallvec![FieldType::Name("timestamp".to_owned())]];
            let timestamp_fmt = Format::new(Some("dimmed"), no_color, compact)?;
            args.push((timestamp, timestamp_fmt));
            pieces.push(Piece::Arg(args.len() - 1));
            pieces.push(Piece::Literal(" ".to_owned()));

            let level = smallvec![
                smallvec![FieldType::Name("level".to_owned())],
                smallvec![FieldType::Name("lvl".to_owned())],
                smallvec![FieldType::Name("severity".to_owned())],
            ];
            let level_fmt = Format::new(Some("level"), no_color, compact)?;
            args.push((level, level_fmt));
            pieces.push(Piece::Arg(args.len() - 1));
            pieces.push(Piece::Literal(" ".to_owned()));

            let message = smallvec![
                smallvec![FieldType::Name("message".to_owned())],
                smallvec![FieldType::Name("msg".to_owned())],
                smallvec![FieldType::Name("body".to_owned())],
            ];
            let message_fmt = Format::new(None, no_color, compact)?;
            args.push((message, message_fmt));
            pieces.push(Piece::Arg(args.len() - 1));
        }
        _ => {
            return Err(FormatError::UnsupportedFunction {
                func: func.to_owned(),
            })
        }
    }

    Ok(())
}

fn parse_field(
    pieces: &mut Vec<Piece>,
    args: &mut Args,
    content: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    // param is a field
    let (name_part, format) = match content.split_once(':') {
        Some((name, styles)) => (name, Format::new(Some(styles), no_color, compact)?),
        None => (content, Format::new(None, no_color, compact)?),
    };

    let mut names = SmallVec::new();
    for name in name_part.split('|') {
        if !name.is_empty() {
            names.push(parse_name(name)?);
        }
    }

    args.push((names, format));
    pieces.push(Piece::Arg(args.len() - 1));

    Ok(())
}

type Args = Vec<(SmallVec<[SmallVec<[FieldType; 4]>; 4]>, Format)>;

#[derive(Debug, Clone)]
pub struct Formatter {
    pieces: Vec<Piece>,
    args: Args,
}

impl Formatter {
    pub fn with_json<'a>(&'a self, json: &'a Json) -> FormatterWithJson<'a> {
        FormatterWithJson {
            formatter: self,
            json,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Piece {
    Literal(String),
    // arg index
    Arg(usize),
    Escaped(char),
}

#[derive(Debug, Clone)]
pub enum FieldType {
    Name(String),
    Index(usize),
}

// parse a name str into list of possible names and/or index
// e.g. "field1.field2[0].field3" -> [Name("field1"), Name("field2"), Index(0), Name("field3")]
fn parse_name(name: &str) -> Result<SmallVec<[FieldType; 4]>, FormatError> {
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

#[derive(Debug, Default, Clone, Copy)]
pub struct Format {
    pub style: Option<Style>,
    pub compact: bool,
    pub is_json: bool,
    pub newline: bool,
    pub indent: usize,
    // special type of modifier only applicable to level field, where the style changes based on the level
    pub is_level: bool,
    pub markup_styles: MarkupStyles,
}

impl Format {
    fn new(input: Option<&str>, no_color: bool, mut compact: bool) -> Result<Self, FormatError> {
        let mut style = (!no_color).then(Style::new);
        let mut is_json = false;
        let mut newline = false;
        let mut indent = 0;
        let mut is_level = false;
        let mut markup_styles = MarkupStyles::default();

        let Some(input) = input else {
            return Ok(Self {
                style,
                compact,
                is_json,
                newline,
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
                    "newline" => {
                        newline = true;
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

        Ok(Self {
            style,
            compact,
            is_json,
            newline,
            indent,
            is_level,
            markup_styles,
        })
    }
}

pub struct FormatterWithJson<'a> {
    formatter: &'a Formatter,
    json: &'a Json<'a>,
}

impl fmt::Display for FormatterWithJson<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Piece::*;

        let Self {
            formatter: Formatter { pieces, args },
            ..
        } = self;

        let mut prev = None;

        for piece in pieces {
            match piece {
                Literal(literal) => write!(f, "{}", literal)?,
                Escaped(c) => write!(f, "{}", c)?,
                Arg(i) => {
                    let (names, format) = &args[*i];

                    let Format {
                        style,
                        compact,
                        is_json,
                        newline,
                        is_level,
                        indent,
                        markup_styles: json_styles,
                    } = format;
                    let indent = *indent;
                    let is_level = *is_level;

                    let mut val = &Json::Null;
                    for args in names.iter() {
                        let mut args = args.iter();
                        if let Some(FieldType::Name(name)) = args.next() {
                            val = self.json.get(name);
                        } else {
                            continue;
                        }

                        for arg in args {
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
                            break;
                        }
                    }

                    if *newline && !val.is_null() {
                        writeln!(f)?;
                        prev = Some(&Escaped('\n'));
                    }

                    match prev {
                        None | Some(&Escaped('\n')) | Some(&Escaped('\r')) => {
                            if indent > 0 {
                                write!(f, "{:indent$}", "", indent = indent)?;
                            }
                        }
                        _ => {}
                    }

                    if let Some(val) = val.as_str() {
                        if let Some(style) = style {
                            if is_level {
                                match val {
                                    "TRACE" | "trace" => write!(
                                        f,
                                        "{}",
                                        val.style((*style).color(AnsiColors::Cyan).dimmed())
                                    )?,
                                    "DEBUG" | "debug" => write!(
                                        f,
                                        "{}",
                                        val.style((*style).color(AnsiColors::Green))
                                    )?,
                                    "INFO" | "info" => write!(
                                        f,
                                        " {}",
                                        val.style((*style).color(AnsiColors::Cyan))
                                    )?,
                                    "WARN" | "warn" => write!(
                                        f,
                                        " {}",
                                        val.style((*style).color(AnsiColors::Yellow))
                                    )?,
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
                    } else if let Some(val) = val.as_value() {
                        if let Some(style) = style.as_ref() {
                            write!(f, "{}", val.style(*style))?;
                        } else {
                            write!(f, "{}", val)?;
                        }
                    } else if val.is_object() || val.is_array() {
                        // TODO: Implement formatting for objects
                        match (is_json, compact) {
                            (true, true) => {
                                if style.is_some() {
                                    write!(f, "{}", val.styled(*json_styles))?;
                                } else {
                                    write!(f, "{}", val)?;
                                }
                            }
                            (true, false) => {
                                if style.is_some() {
                                    write!(f, "{:?}", val.indented(indent).styled(*json_styles))?;
                                } else {
                                    write!(f, "{:?}", val.indented(indent))?;
                                }
                            }
                            (false, true) => {
                                if style.is_some() {
                                    write!(f, "{}", val.styled(*json_styles))?;
                                } else {
                                    write!(f, "{}", val)?;
                                }
                            }
                            (false, false) => {
                                if style.is_some() {
                                    write!(f, "{:?}", val.indented(indent).styled(*json_styles))?;
                                } else {
                                    write!(f, "{:?}", val.indented(indent))?;
                                }
                            }
                        }
                    }
                }
            }

            prev = Some(piece);
        }

        Ok(())
    }
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
