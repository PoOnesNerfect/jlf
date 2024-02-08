use core::fmt;
use owo_colors::DynColors;

use crate::{json::JsonStyles, Json};

pub use owo_colors::{OwoColorize as Colorize, Style};

// Example log format
// '{timestamp:fg=green} {level:fg=blue} {message} {#for span in spans}{span.name}{/for}'
pub fn parse_formatter(input: &str, no_color: bool) -> Result<Formatter, FormatError> {
    let mut pieces = Vec::new();

    let mut parts = input.split('\\');

    if let Some(part) = parts.next() {
        parse2(&mut pieces, part, no_color)?;
    }

    for part in parts {
        let (escaped, rest) = part.split_at(1);
        pieces.push(parse_escaped(escaped.chars().next().unwrap())?);
        parse2(&mut pieces, rest, no_color)?;
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

fn parse2(pieces: &mut Vec<Piece>, input: &str, no_color: bool) -> Result<(), FormatError> {
    let mut parts = input.split('{');

    if let Some(part) = parts.next() {
        if !part.is_empty() {
            pieces.push(Piece::Literal(part.to_owned()));
        }
    }

    for part in parts {
        if let Some(end) = part.find('}') {
            let (name, format) = match part[..end].split_once(':') {
                Some((name, styles)) => (name.to_string(), Format::new(Some(styles), no_color)?),
                None => (part[..end].to_string(), Format::new(None, no_color)?),
            };

            pieces.push(Piece::Arg(name, format));

            let literal = &part[end + 1..];
            if !literal.is_empty() {
                pieces.push(Piece::Literal(literal.to_owned()));
            }
        } else {
            if !part.is_empty() {
                pieces.push(Piece::Literal(part.to_owned()));
            }
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
    Arg(String, Format),
    Escaped(char),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Format {
    pub style: Option<Style>,
    pub compact: bool,
    pub json: bool,
    pub indent: usize,
    pub json_styles: JsonStyles,
}

impl Format {
    fn new(input: Option<&str>, no_color: bool) -> Result<Self, FormatError> {
        let mut style = (!no_color).then(|| Style::new());
        let mut compact = false;
        let mut json = false;
        let mut indent = 0;
        let mut json_styles = JsonStyles::default();

        let Some(input) = input else {
            return Ok(Self {
                style,
                compact,
                json,
                indent,
                json_styles,
            });
        };

        for part in input.split(',') {
            let (name, value) = if let Some((name, value)) = part.split_once('=') {
                (name, value)
            } else {
                if part == "compact" {
                    compact = true;
                    continue;
                } else if part == "json" {
                    json = true;
                    continue;
                }

                ("fg", part)
            };

            match name {
                "fg" => {
                    let Ok(color) = value.parse::<DynColors>() else {
                        return Err(FormatError::ParseColor(value.to_owned()));
                    };
                    if let Some(s) = style.take() {
                        style = Some(s.color(color));
                    }
                }
                "bg" => {
                    let Ok(color) = value.parse::<DynColors>() else {
                        return Err(FormatError::ParseColor(value.to_owned()));
                    };
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
                    let Ok(color) = value.parse::<DynColors>() else {
                        return Err(FormatError::InvalidModifier(value.to_owned()));
                    };
                    json_styles.key = json_styles.key.color(color);
                }
                "value" => {
                    let Ok(color) = value.parse::<DynColors>() else {
                        return Err(FormatError::InvalidModifier(value.to_owned()));
                    };
                    json_styles.value = json_styles.value.color(color);
                }
                "str" => {
                    let Ok(color) = value.parse::<DynColors>() else {
                        return Err(FormatError::InvalidModifier(value.to_owned()));
                    };
                    json_styles.str = json_styles.str.color(color);
                }
                "syntax" => {
                    let Ok(color) = value.parse::<DynColors>() else {
                        return Err(FormatError::InvalidModifier(value.to_owned()));
                    };
                    json_styles.syntax = json_styles.syntax.color(color);
                }
                _ => return Err(FormatError::InvalidModifier(name.to_owned())),
            }
        }

        Ok(Self {
            style,
            compact,
            json,
            indent,
            json_styles,
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
                Arg(name, format) => {
                    let Format {
                        style,
                        compact,
                        json,
                        indent,
                        json_styles,
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

                    let val = self.json.get(name);
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
                        match (json, compact) {
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
                        match (json, compact) {
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
    #[error("Invalid color '{0}'")]
    ParseColor(String),
    #[error("Invalid indent value '{0}'")]
    ParseIndent(String),
    #[error("Invalid modifier '{0}'")]
    InvalidModifier(String),
    #[error("Unknown character escape '\\{0}'")]
    UnknownCharEscape(char),
}
