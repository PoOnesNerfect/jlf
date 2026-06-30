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
    /// Whether any arg is an optional `{?field}` — gates the (slightly costlier)
    /// whitespace-collapsing render path so plain templates pay nothing.
    has_optional: bool,
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

        let has_optional = args.iter().any(|(_, fmt)| fmt.optional);

        Ok(Formatter {
            pieces,
            args,
            has_optional,
        })
    }

    /// Apply an escape (e.g. HTML) to every interpolated value — used by custom
    /// `[format.*]` output formats so their `row` template is safe.
    pub fn with_escape(mut self, escape: Escape) -> Self {
        for (_, fmt) in &mut self.args {
            fmt.escape = escape;
        }
        self
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
    // `{?field}`: when this field renders empty, collapse one adjacent space so
    // an absent field leaves no stray gap.
    pub optional: bool,
    // escape applied to interpolated values (custom output formats), e.g. HTML.
    pub escape: Escape,
    pub markup_styles: MarkupStyles,
}

/// How interpolated field values are escaped — used by custom `[format.*]`
/// output formats to make values safe in structured text (e.g. HTML).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    #[default]
    None,
    Html,
}

impl Escape {
    /// Parse an `escape = "..."` config value. Returns `None` for unknown names.
    pub fn from_name(name: &str) -> Option<Escape> {
        match name {
            "" | "none" => Some(Escape::None),
            "html" => Some(Escape::Html),
            _ => None,
        }
    }
}
