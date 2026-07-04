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
pub enum Field {
    Names(FieldNames),
    Whole,
    Rest,
    /// `$value` inside a `$( … )` repetition: the current entry's value.
    ColValue,
    /// `${value.path}` inside a repetition: a sub-path of the entry's value.
    ColValuePath(FieldNames),
    /// `$key` inside a `$( … )` repetition: the current entry's key/name.
    ColKey,
}

/// The source a `$( … )` repetition iterates over.
#[derive(Debug, Clone, PartialEq)]
pub enum RepSource {
    /// Bare `$( … )`: iterate the current record's top-level fields.
    Record,
    /// `$cols( … )`: iterate the CLI-selected columns (`-f`/args).
    Columns,
    /// `$path( … )`: iterate the object/array at `path` in the record.
    Path(Field),
}

/// A column: a display `name` plus the parsed accessor used to pull its value.
#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    field: Field,
}

/// Build a [`Column`] from a field name/path (e.g. `ts`, `user.id`).
pub fn column(name: impl Into<String>) -> Column {
    let name = name.into();
    let field = parse::parse_field(&name).unwrap_or(Field::Whole);
    Column { name, field }
}

#[derive(Debug, Clone)]
pub struct Formatter {
    pieces: Vec<Piece>,
    args: Vec<Arg>,
    /// Whether any arg is an optional `{?field}` — gates the (slightly costlier)
    /// whitespace-collapsing render path so plain templates pay nothing.
    has_optional: bool,
    /// Columns for bare `$( … )` repetition (from `-f`/args); empty otherwise.
    columns: Vec<Column>,
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
            columns: Vec::new(),
        })
    }

    /// Apply an escape (e.g. HTML) to every interpolated value — used by custom
    /// output formats so their body template is safe.
    pub fn with_escape(mut self, escape: Escape) -> Self {
        for (_, fmt) in &mut self.args {
            fmt.escape = escape;
        }
        self
    }

    /// Set the columns iterated by a bare `$( … )` repetition.
    pub fn with_columns(mut self, columns: Vec<Column>) -> Self {
        self.columns = columns;
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
    CondStart(Cond, usize),
    ElseCond(Cond, usize),
    Else,
    CondEnd,
    /// Start of a `$( … )sep op` repetition: what to iterate, the separator
    /// emitted between iterations, and the repetition operator.
    RepStart(RepSource, String, RepOp),
    /// End of a `$( … )` repetition.
    RepEnd,
}

/// The repetition operator on a `$( … )` group: `*` (zero or more), `+` (one or
/// more), `?` (zero or one). All iterate the selected column list; the operator
/// documents intent (kept for parity with `macro_rules!`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepOp {
    Star,
    Plus,
    Question,
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

/// How interpolated field values are escaped for the surrounding output format.
/// `Html` makes values safe in HTML; `Csv`/`Tsv`/`Md` quote or clean cells for
/// those table dialects. This single enum replaces the old split between a
/// framed-format `Escape` and a table `Quote`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    #[default]
    None,
    Html,
    /// RFC-4180 CSV: wrap in quotes when the cell contains `"`, a newline, or a
    /// comma; double any inner quotes.
    Csv,
    /// TSV: replace tabs and newlines with spaces.
    Tsv,
    /// Markdown table cell: escape `|` and turn newlines into `<br>`.
    Md,
}

impl Escape {
    /// Parse an `escape`/modifier name. Returns `None` for unknown names.
    pub fn from_name(name: &str) -> Option<Escape> {
        match name {
            "" | "none" => Some(Escape::None),
            "html" => Some(Escape::Html),
            "csv" => Some(Escape::Csv),
            "tsv" => Some(Escape::Tsv),
            "md" => Some(Escape::Md),
            _ => None,
        }
    }
}

#[cfg(test)]
mod dsl_tests {
    use super::*;
    use crate::{column, expanded_format, Json};

    fn render(tpl: &str, cols: &[&str], json_str: &str) -> String {
        let cols: Vec<_> = cols.iter().map(|c| column(*c)).collect();
        let f = Formatter::new(tpl, true, false).unwrap().with_columns(cols);
        let mut j = Json::Null;
        j.parse_replace(json_str).unwrap();
        let mut out = String::new();
        f.as_log(&j).write_fmt(&mut out).unwrap();
        out
    }

    #[test]
    fn bare_field_and_path() {
        assert_eq!(render("$level $ts", &[], r#"{"level":"INFO","ts":"1"}"#), "INFO 1");
        assert_eq!(render("${user.id}", &[], r#"{"user":{"id":"7"}}"#), "7");
    }

    #[test]
    fn column_rep_csv() {
        assert_eq!(
            render(r#"$cols( $key ),*"#, &["ts", "level"], r#"{"ts":"1","level":"INFO"}"#),
            "ts,level"
        );
        assert_eq!(
            render(r#"$cols( ${value:csv} ),*"#, &["ts", "level"], r#"{"ts":"1","level":"a,b"}"#),
            "1,\"a,b\""
        );
    }

    #[test]
    fn record_rep_flattens_root() {
        assert_eq!(
            render(r#"$( $key=$value )" "*"#, &[], r#"{"a":"1","b":"2"}"#),
            "a=1 b=2"
        );
    }

    #[test]
    fn md_row_with_quoted_sep() {
        assert_eq!(
            render(r#"| $cols( ${value:md} )" | "* |"#, &["a", "b"], r#"{"a":"x","b":"y"}"#),
            "| x | y |"
        );
    }

    #[test]
    fn path_rep_flatten_fields() {
        assert_eq!(
            render(r#"$fields( $key=$value )" "*"#, &[], r#"{"fields":{"dir":"/d","n":5}}"#),
            "dir=/d n=5"
        );
    }

    #[test]
    fn conditional_and_optional() {
        assert_eq!(
            render("${if warn}W${else}ok${/}", &[], r#"{"warn":true}"#),
            "W"
        );
        assert_eq!(render("a${?missing} b", &[], r#"{}"#), "ab");
    }

    #[test]
    fn include_expands() {
        let vars = vec![("lvl".to_string(), "${level:level}".to_string())];
        let expanded = expanded_format("[${@lvl}]", &vars);
        assert_eq!(expanded, "[${level:level}]");
    }
    #[test]
    fn value_path_in_array_rep() {
        assert_eq!(
            render(r#"$spans( [${value.name}] )" "*"#, &[], r#"{"spans":[{"name":"a"},{"name":"b"}]}"#),
            "[a] [b]"
        );
    }
    #[test]
    fn rep_skips_consumed_key() {
        // referencing fields.message first drops it from the $fields flatten
        assert_eq!(
            render(r#"${fields.message} | $fields( $key=$value )" "*"#, &[],
                r#"{"fields":{"message":"hi","a":"1","b":"2"}}"#),
            "hi | a=1 b=2"
        );
    }

    #[test]
    fn rep_body_keeps_newlines() {
        assert_eq!(
            render("head$( \n  $key )*", &[], r#"{"a":"1","b":"2"}"#),
            "head\n  a\n  b"
        );
    }
}
