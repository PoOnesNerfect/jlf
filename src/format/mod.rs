use core::fmt;

use owo_colors::AnsiColors;
pub use owo_colors::{OwoColorize as Colorize, Style};
use smallvec::SmallVec;

use crate::{json::MarkupStyles, Json};

mod parse;
pub use parse::parse_formatter;

type Arg = (FieldOptions, Format);
type FieldOptions = SmallVec<[Field; 2]>;
type Field = SmallVec<[FieldType; 2]>;

#[derive(Debug, Clone)]
pub struct Formatter {
    pieces: Vec<Piece>,
    args: Vec<Arg>,
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
    CondStart(Cond, usize),
    ElseCond(Cond, usize),
    Else,
    CondEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    If,
    Key,
}

#[derive(Debug, Clone)]
pub enum FieldType {
    Name(String),
    Index(usize),
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Format {
    pub style: Option<Style>,
    pub compact: bool,
    pub is_json: bool,
    pub indent: usize,
    // special type of modifier only applicable to level field, where the style
    // changes based on the level
    pub is_level: bool,
    pub markup_styles: MarkupStyles,
}

// used for displaying the formatted log to output
pub struct FormatterWithJson<'a> {
    formatter: &'a Formatter,
    json: &'a Json<'a>,
}

impl fmt::Display for FormatterWithJson<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            formatter: Formatter { pieces, args },
            json,
        } = self;

        let mut prev = None;

        let mut piece_i = 0;
        while piece_i < pieces.len() {
            piece_i = write_piece(f, pieces, piece_i, args, json, &mut prev)?;
        }

        Ok(())
    }
}

fn write_piece(
    f: &mut fmt::Formatter<'_>,
    pieces: &Vec<Piece>,
    mut piece_i: usize,
    args: &Vec<Arg>,
    json: &Json<'_>,
    prev: &mut Option<&Piece>,
) -> Result<usize, fmt::Error> {
    use Piece::*;

    match &pieces[piece_i] {
        Literal(literal) => write!(f, "{}", literal)?,
        Escaped(c) => write!(f, "{}", c)?,
        Arg(i) => write_arg(f, &args[*i], json, prev)?,
        CondStart(cond, i) => {
            let mut should_run = test_cond(*cond, args, *i, json);
            let mut else_found = false;

            piece_i += 1;
            while piece_i < pieces.len() {
                if let Piece::ElseCond(cond, i) = pieces[piece_i] {
                    should_run = !should_run && !else_found && test_cond(cond, args, i, json);

                    piece_i += 1;
                    continue;
                } else if let Piece::Else = pieces[piece_i] {
                    should_run = !should_run && !else_found;
                    else_found = true;

                    piece_i += 1;
                    continue;
                } else if let Piece::CondEnd = pieces[piece_i] {
                    break;
                }

                if should_run {
                    piece_i = write_piece(f, pieces, piece_i, args, json, prev)?;
                } else {
                    piece_i += 1;
                }
            }
        }
        // Handled in the IfStart case above
        ElseCond(..) | Else | CondEnd => {}
    }

    Ok(piece_i + 1)
}

fn test_cond(cond: Cond, args: &[Arg], i: usize, json: &Json<'_>) -> bool {
    let (field_options, _) = &args[i];
    let mut val = &Json::Null;
    for field in field_options {
        val = json;
        for arg in field {
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

    if val.is_null() {
        return false;
    }

    match cond {
        Cond::Key => true,
        Cond::If => {
            if val.is_array() || val.is_object() {
                true
            } else if let Some(val) = val.as_str() {
                !val.is_empty()
            } else if let Some(val) = val.as_value() {
                !(val == "false"
                    || val == "0"
                    || val == "-0"
                    || val == "0n"
                    || val == "undefined"
                    || val == "NaN")
            } else {
                unreachable!("all cases checked")
            }
        }
    }
}

fn write_arg(
    f: &mut fmt::Formatter<'_>,
    (field_options, format): &(FieldOptions, Format),
    json: &Json<'_>,
    prev: &mut Option<&Piece>,
) -> fmt::Result {
    use Piece::*;

    let Format {
        style,
        compact,
        is_json,
        is_level,
        indent,
        markup_styles: json_styles,
    } = format;
    let indent = *indent;
    let is_level = *is_level;

    let mut val = &Json::Null;

    // if field is a single empty string, then the whole json is used
    for field in field_options {
        val = json;
        for arg in field {
            match arg {
                FieldType::Name(name) => {
                    if name.is_empty() {
                        val = json;
                        break;
                    }
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

    Ok(())
}
