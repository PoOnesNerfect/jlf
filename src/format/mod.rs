use smallvec::SmallVec;

use crate::{json::MarkupStyles, Json};

mod log;
pub mod parse;

pub use log::FormattedLog;
pub use owo_colors::{OwoColorize as Colorize, Style};

type Arg = (FieldOptions, Format);
type FieldOptions = SmallVec<[Field; 2]>;
type FieldNames = SmallVec<[FieldType; 2]>;

#[derive(Debug, Clone, PartialEq)]
enum Field {
    Names(FieldNames),
    Whole,
    Rest,
}

#[derive(Debug, Clone)]
pub struct Formatter {
    pieces: Vec<Piece>,
    args: Vec<Arg>,
}

impl Formatter {
    pub fn new(
        input: &str,
        no_color: bool,
        compact: bool,
    ) -> Result<Formatter, parse::FormatError> {
        let mut pieces = Vec::new();
        let mut args = Vec::new();

        parse::crunch_input(&mut pieces, &mut args, input, no_color, compact)?;

        Ok(Formatter { pieces, args })
    }

    pub fn as_log<'a>(&'a self, json: &'a Json<'a>) -> FormattedLog<'a> {
        FormattedLog {
            formatter: self,
            json,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    IfConfig(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    Name(String),
    Index(usize),
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
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
