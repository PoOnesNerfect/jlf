use core::fmt;
use smallvec::SmallVec;

use crate::{
    colors::{parse_color, ParseColorError},
    json::MarkupStyles,
    Json,
};

pub use owo_colors::{OwoColorize as Colorize, Style};

// Example log format
// '{timestamp:fg=green} {level:fg=blue} {message} {#for span in spans}{span.name}{/for}'
pub fn parse_formatter(
    input: &str,
    no_color: bool,
    compact: bool,
) -> Result<Formatter, FormatError> {
    let mut pieces = Vec::new();

    let mut parts = input.split('\\');

    if let Some(part) = parts.next() {
        parse2(&mut pieces, part, no_color, compact)?;
    }

    for part in parts {
        let (escaped, rest) = part.split_at(1);
        pieces.push(parse_escaped(escaped.chars().next().unwrap())?);
        parse2(&mut pieces, rest, no_color, compact)?;
    }

    Ok(Formatter { pieces })
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

fn parse2(
    pieces: &mut Vec<Piece>,
    input: &str,
    no_color: bool,
    compact: bool,
) -> Result<(), FormatError> {
    let mut parts = input.split('{');

    if let Some(part) = parts.next() {
        if !part.is_empty() {
            pieces.push(Piece::Literal(part.to_owned()));
        }
    }

    for part in parts {
        if let Some(end) = part.find('}') {
            let (name_part, format) = match part[..end].split_once(':') {
                Some((name, styles)) => (name, Format::new(Some(styles), no_color, compact)?),
                None => (&part[..end], Format::new(None, no_color, compact)?),
            };

            let mut names = SmallVec::new();
            for name in name_part.split('|') {
                if !name.is_empty() {
                    names.push(name.to_owned());
                }
            }

            pieces.push(Piece::Arg(names, format));

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

#[derive(Debug, Clone)]
pub struct Formatter {
    pieces: Vec<Piece>,
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
    // (names, format)
    Arg(SmallVec<[String; 8]>, Format),
    Escaped(char),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Format {
    pub style: Option<Style>,
    pub compact: bool,
    pub is_json: bool,
    pub indent: usize,
    pub markup_styles: MarkupStyles,
}

impl Format {
    fn new(input: Option<&str>, no_color: bool, mut compact: bool) -> Result<Self, FormatError> {
        let mut style = (!no_color).then(|| Style::new());
        let mut is_json = false;
        let mut indent = 0;
        let mut markup_styles = MarkupStyles::default();

        let Some(input) = input else {
            return Ok(Self {
                style,
                compact,
                is_json,
                indent,
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
                "ident" => {
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
            indent,
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
            formatter: Formatter { pieces },
            ..
        } = self;

        let mut prev = None;

        for piece in pieces {
            match piece {
                Literal(literal) => write!(f, "{}", literal)?,
                Escaped(c) => write!(f, "{}", c)?,
                Arg(names, format) => {
                    let Format {
                        style,
                        compact,
                        is_json,
                        indent,
                        markup_styles: json_styles,
                    } = format;
                    let indent = *indent;

                    match prev {
                        None | Some(&Escaped('\n')) | Some(&Escaped('\r')) => {
                            if indent > 0 {
                                write!(f, "{:indent$}", "", indent = indent)?;
                            }
                        }
                        _ => {}
                    }

                    let mut val = &Json::Null;
                    for name in names.iter() {
                        val = self.json.get(name);
                        if !val.is_null() {
                            break;
                        }
                    }

                    if let Some(val) = val.as_str() {
                        if let Some(style) = style {
                            write!(f, "{}", val.style(*style))?;
                        } else {
                            write!(f, "{}", val)?;
                        }
                    } else if let Some(val) = val.as_value() {
                        if let Some(style) = style.as_ref() {
                            write!(f, "{}", val.style(*style))?;
                        } else {
                            write!(f, "{}", val)?;
                        }
                    } else if val.is_object() {
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
                    } else if val.is_array() {
                        // TODO: Implement formatting for arrays
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
}
