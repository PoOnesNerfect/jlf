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
    /// A quoted literal (`${"text":mods}`): constant text rendered with the
    /// arg's modifiers, so raw text can be styled/escaped like a value.
    Literal(String),
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
    /// Whether any arg is an optional `{?field}` — gates the (slightly
    /// costlier) whitespace-collapsing render path so plain templates pay
    /// nothing.
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
    /// Start of a `${match path}` block: `path` is the subject arg. The
    /// subject's value is bound to `$value` for the arms in between, up to
    /// `MatchEnd`.
    MatchStart(usize),
    /// End of a `${match … }` block.
    MatchEnd,
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

#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    If,
    Has,
    IfConfig(bool),
    /// `$if(field OP literal => …)` — compare the field's scalar value against
    /// a literal. Numeric when both sides parse as numbers, else
    /// lexicographic.
    Cmp(CmpOp, String),
    /// A `${match}` arm: matches when the bound subject (`$value`) satisfies
    /// any of the listed tests. An empty list is the `_` wildcard — it
    /// matches any present (non-null) value, so a missing subject renders
    /// nothing.
    Arm(Vec<ArmTest>),
}

/// One alternative in a `${match}` arm pattern (`a | b | …`).
#[derive(Debug, Clone, PartialEq)]
pub enum ArmTest {
    /// A comparison against the subject, e.g. `>= 500` or `== "GET"`.
    Cmp(CmpOp, String),
    /// A numeric range, e.g. `400..500` (half-open) or `500..` (from). `hi` is
    /// inclusive only for `..=` forms.
    Range {
        lo: Option<f64>,
        hi: Option<f64>,
        hi_inclusive: bool,
    },
}

/// Comparison operator for an `$if(field OP literal => …)` condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
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
    // `{?field}`: when this field renders empty, collapse one adjacent space
    // so an absent field leaves no stray gap.
    pub optional: bool,
    // escape applied to interpolated values (custom output formats), e.g.
    // HTML.
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
    /// RFC-4180 CSV: wrap in quotes when the cell contains `"`, a newline, or
    /// a comma; double any inner quotes.
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
        assert_eq!(
            render("$level $ts", &[], r#"{"level":"INFO","ts":"1"}"#),
            "INFO 1"
        );
        assert_eq!(render("${user.id}", &[], r#"{"user":{"id":"7"}}"#), "7");
    }

    #[test]
    fn flattened_dotted_key_falls_back() {
        // A literal dotted key resolves when there's no matching nesting.
        assert_eq!(
            render(
                "${fields.log.file}",
                &[],
                r#"{"fields":{"log.file":"/p"}}"#
            ),
            "/p"
        );
        // A top-level flattened key too.
        assert_eq!(render("${a.b}", &[], r#"{"a.b":"flat"}"#), "flat");
        // Real nesting still wins over a same-named flattened key.
        assert_eq!(
            render("${a.b}", &[], r#"{"a":{"b":"nested"},"a.b":"flat"}"#),
            "nested"
        );
    }

    #[test]
    fn used_flattened_key_is_excluded_from_repetition() {
        // A flattened key rendered up top must be dropped from a later
        // `$fields( … )` breakdown (not double-shown).
        assert_eq!(
            render(
                r#"${fields.log.file} $fields( ${key}=${value} )" "*"#,
                &[],
                r#"{"fields":{"log.file":"/p","other":"x"}}"#,
            ),
            "/p other=x"
        );
    }

    #[test]
    fn malformed_repetition_errors_and_is_not_swallowed() {
        // An operator-less `$name( … )` must error even when a valid repetition
        // follows on the next line — previously the bare-separator scan
        // absorbed the following block, silently rendering nothing.
        assert!(
            Formatter::new("$key(a => b)\n$fields( $key )*", true, false)
                .is_err()
        );
        // A real bare separator right after `)` still parses.
        assert!(Formatter::new("$cols( $key ),*", true, false).is_ok());
    }

    #[test]
    fn column_rep_csv() {
        assert_eq!(
            render(
                r#"$cols( $key ),*"#,
                &["ts", "level"],
                r#"{"ts":"1","level":"INFO"}"#
            ),
            "ts,level"
        );
        assert_eq!(
            render(
                r#"$cols( ${value:csv} ),*"#,
                &["ts", "level"],
                r#"{"ts":"1","level":"a,b"}"#
            ),
            "1,\"a,b\""
        );
    }

    #[test]
    fn object_cell_is_compact_and_escaped() {
        // A nested object in a table cell must stay on one line and be quoted
        // for the dialect — never pretty-printed across rows.
        assert_eq!(
            render(
                r#"$cols( ${value:csv} ),*"#,
                &["fields"],
                r#"{"fields":{"m":"a,b","n":5}}"#
            ),
            r#""{""m"":""a,b"",""n"":5}""#
        );
        assert_eq!(
            render(
                r#"$cols( ${value:md} )" | "*"#,
                &["fields"],
                r#"{"fields":{"m":"x"}}"#
            ),
            r#"{"m":"x"}"#
        );
        assert_eq!(
            render(
                r#"$cols( ${value:tsv} )\t*"#,
                &["fields"],
                r#"{"fields":{"m":"x"}}"#
            ),
            r#"{"m":"x"}"#
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
            render(
                r#"| $cols( ${value:md} )" | "* |"#,
                &["a", "b"],
                r#"{"a":"x","b":"y"}"#
            ),
            "| x | y |"
        );
    }

    #[test]
    fn path_rep_flatten_fields() {
        assert_eq!(
            render(
                r#"$fields( $key=$value )" "*"#,
                &[],
                r#"{"fields":{"dir":"/d","n":5}}"#
            ),
            "dir=/d n=5"
        );
    }

    #[test]
    fn conditional_and_optional() {
        assert_eq!(
            render("$if(warn => W)$else(ok)", &[], r#"{"warn":true}"#),
            "W"
        );
        assert_eq!(render("a${?missing} b", &[], r#"{}"#), "ab");
    }

    #[test]
    fn if_comparison_numeric() {
        let t = "$if(status >= 500 => 5xx)$elif(status >= 400 => 4xx)$else(ok)";
        assert_eq!(render(t, &[], r#"{"status":200}"#), "ok");
        assert_eq!(render(t, &[], r#"{"status":429}"#), "4xx");
        assert_eq!(render(t, &[], r#"{"status":503}"#), "5xx");
    }

    #[test]
    fn if_comparison_operators() {
        assert_eq!(
            render("$if(n == 200 => y)$else(n)", &[], r#"{"n":200}"#),
            "y"
        );
        assert_eq!(
            render("$if(n != 200 => y)$else(n)", &[], r#"{"n":201}"#),
            "y"
        );
        assert_eq!(render("$if(n <= 3 => y)$else(n)", &[], r#"{"n":3}"#), "y");
        assert_eq!(render("$if(n < 3 => y)$else(n)", &[], r#"{"n":3}"#), "n");
    }

    #[test]
    fn if_comparison_string_literal() {
        let t = r#"$if(method == "GET" => read)$else(write)"#;
        assert_eq!(render(t, &[], r#"{"method":"GET"}"#), "read");
        assert_eq!(render(t, &[], r#"{"method":"POST"}"#), "write");
    }

    #[test]
    fn if_comparison_missing_field_is_false() {
        // an absent or non-scalar field never satisfies a comparison
        assert_eq!(render("$if(n >= 1 => y)$else(n)", &[], r#"{}"#), "n");
    }

    #[test]
    fn has_vs_if_and_config() {
        // `$has` tests existence, `$if` truthiness — so a present `0` differs
        assert_eq!(
            render("$has(body => has)$else(no)", &[], r#"{"body":0}"#),
            "has"
        );
        assert_eq!(
            render("$if(body => yes)$else(no)", &[], r#"{"body":0}"#),
            "no"
        );
        // an all-whitespace branch body is kept as an intentional separator
        assert_eq!(
            render("a$config(compact =>  )$else(\n)b", &[], r#"{}"#),
            "a\nb"
        );
    }

    #[test]
    fn match_ranges() {
        let t = "$match(status $when(500.. => 5xx) $when(400..500 => 4xx) \
                 $else(ok))";
        assert_eq!(render(t, &[], r#"{"status":200}"#), "ok");
        assert_eq!(render(t, &[], r#"{"status":404}"#), "4xx");
        assert_eq!(render(t, &[], r#"{"status":503}"#), "5xx");
        // a missing subject matches no arm — not even `$else`
        assert_eq!(render(t, &[], r#"{}"#), "");
    }

    #[test]
    fn match_value_binding_and_alternation() {
        let t = r#"$match(method $when("GET"|"HEAD" => read:$value) $else(other:$value))"#;
        assert_eq!(render(t, &[], r#"{"method":"GET"}"#), "read:GET");
        assert_eq!(render(t, &[], r#"{"method":"HEAD"}"#), "read:HEAD");
        assert_eq!(render(t, &[], r#"{"method":"POST"}"#), "other:POST");
    }

    #[test]
    fn match_subject_fallback() {
        let t = r#"$match(lvl|level $when("ERR" => !) $else($value))"#;
        assert_eq!(render(t, &[], r#"{"level":"info"}"#), "info");
        assert_eq!(render(t, &[], r#"{"lvl":"ERR"}"#), "!");
    }

    #[test]
    fn match_multiline_layout() {
        // decorative line breaks + indentation around arms are dropped
        let t = "$match(lvl\n  $when(\"INFO\" => info)\n  $when(\"ERROR\" => \
                 err)\n)";
        assert_eq!(render(t, &[], r#"{"lvl":"INFO"}"#), "info");
        assert_eq!(render(t, &[], r#"{"lvl":"ERROR"}"#), "err");
    }

    #[test]
    fn match_on_repetition_value() {
        // `$match(value …)` inside `$( … )` dispatches on each entry's value
        let t =
            r#"$( $key=$match(value $when(>=100 => big) $else(small)) )" "*"#;
        assert_eq!(render(t, &[], r#"{"a":5,"b":500}"#), "a=small b=big");
    }

    #[test]
    fn nested_match() {
        let t = "$match(s $when(>=500 => $match(value $when(==503 => down) \
                 $else(5xx))) $else(ok))";
        assert_eq!(render(t, &[], r#"{"s":200}"#), "ok");
        assert_eq!(render(t, &[], r#"{"s":500}"#), "5xx");
        assert_eq!(render(t, &[], r#"{"s":503}"#), "down");
    }

    #[test]
    fn include_expands() {
        let vars = vec![("lvl".to_string(), "${level:dimmed}".to_string())];
        let expanded = expanded_format("[${@lvl}]", &vars);
        assert_eq!(expanded, "[${level:dimmed}]");
    }
    #[test]
    fn value_path_in_array_rep() {
        assert_eq!(
            render(
                r#"$spans( [${value.name}] )" "*"#,
                &[],
                r#"{"spans":[{"name":"a"},{"name":"b"}]}"#
            ),
            "[a] [b]"
        );
    }
    #[test]
    fn rep_skips_consumed_key() {
        // referencing fields.message first drops it from the $fields flatten
        assert_eq!(
            render(
                r#"${fields.message} | $fields( $key=$value )" "*"#,
                &[],
                r#"{"fields":{"message":"hi","a":"1","b":"2"}}"#
            ),
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
    #[test]
    fn if_guard_block() {
        let t = r#"${level}$if(span.method => -> ${span.method})"#;
        assert_eq!(
            render(t, &[], r#"{"level":"INFO","span":{"method":"GET"}}"#),
            "INFO-> GET"
        );
        assert_eq!(render(t, &[], r#"{"level":"WARN"}"#), "WARN");
    }
    #[test]
    fn quoted_literal_renders_constant_text() {
        // A quoted hole is literal text, not a field lookup.
        assert_eq!(
            render(r#"${"hi"} ${x}"#, &[], r#"{"x":"there"}"#),
            "hi there"
        );
        // Modifiers apply to it (escaping is observable without color).
        assert_eq!(render(r#"${"a,b":csv}"#, &[], r#"{}"#), "\"a,b\"");
        // A colon inside the literal is not a modifier separator.
        assert_eq!(render(r#"${"a:b"}"#, &[], r#"{}"#), "a:b");
        // Escapes: \t, \", and a literal backslash.
        assert_eq!(render(r#"${"x\ty\"z"}"#, &[], r#"{}"#), "x\ty\"z");
        // Single-quoted literals work too.
        assert_eq!(render(r#"${'lit'}"#, &[], r#"{}"#), "lit");
    }

    #[test]
    fn quoted_literal_errors() {
        let bad = |t: &str| Formatter::new(t, true, false).is_err();
        assert!(bad(r#"${"oops}"#)); // unterminated
        assert!(bad(r#"${"x" foo}"#)); // trailing junk after the literal
    }

    #[test]
    fn ordering_comparisons_tolerate_unit_suffixes() {
        // A numeric value with a unit suffix must compare by magnitude, not
        // lexicographically. `$when(<150)` and a `$if` ordering op both apply.
        let arm = r#"$match(fields.latency $when(<150 => lo) $else(hi))"#;
        let lo = |v: &str| {
            render(arm, &[], &format!(r#"{{"fields":{{"latency":"{v}"}}}}"#))
        };
        assert_eq!(lo("17.881 ms"), "lo"); // was "hi" (lexicographic bug)
        assert_eq!(lo("8.972 ms"), "lo"); // was "hi"
        assert_eq!(lo("144.306 ms"), "lo");
        assert_eq!(lo("236.643 ms"), "hi");
        // Ranges tolerate the suffix too.
        let rng = r#"$match(fields.latency $when(0..150 => lo) $else(hi))"#;
        assert_eq!(
            render(rng, &[], r#"{"fields":{"latency":"17.881 ms"}}"#),
            "lo"
        );
        // `$if` ordering op.
        let cond = r#"$if(d > 100 => big)$else(small)"#;
        assert_eq!(render(cond, &[], r#"{"d":"17.881 ms"}"#), "small");
        assert_eq!(render(cond, &[], r#"{"d":"236.6 ms"}"#), "big");
    }

    #[test]
    fn equality_stays_a_string_compare() {
        // Equality must not go numeric on values that merely start with digits,
        // or "2xx" would wrongly equal "2yy".
        let t = r#"$if(code == "2xx" => match)$else(no)"#;
        assert_eq!(render(t, &[], r#"{"code":"2xx"}"#), "match");
        assert_eq!(render(t, &[], r#"{"code":"2yy"}"#), "no");
    }
}
