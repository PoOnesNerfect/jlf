use crate::Json;

/// A single `key OP value[,value...]` predicate over a parsed record.
#[derive(Debug, Clone)]
pub struct Filter {
    /// Fallback field paths (`a|b.c` -> `[[a], [b, c]]`); first present wins.
    paths: Vec<Vec<String>>,
    op: Op,
    values: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Gt,
    Lt,
    Ge,
    Le,
    Contains,
    NotContains,
}

impl Filter {
    /// The dotted field path(s) this filter tests (e.g. `data.user.id`, or
    /// `lvl|level|severity` for a fallback). Used to decide when an explicit
    /// filter overrides a preset's filter on the same field.
    pub fn key(&self) -> String {
        self.paths
            .iter()
            .map(|p| p.join("."))
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Parse a token like `level=error`, `latency>500`, `msg~timeout`,
    /// `level=error,warn`, or `lvl|level|severity=error` (fallback fields).
    /// Returns `None` if the token has no operator.
    pub fn parse(token: &str) -> Option<Filter> {
        // order matters: check 2-char ops before 1-char
        let ops: &[(&str, Op)] = &[
            ("!=", Op::Ne),
            (">=", Op::Ge),
            ("<=", Op::Le),
            ("!~", Op::NotContains),
            ("~", Op::Contains),
            ("=", Op::Eq),
            (">", Op::Gt),
            ("<", Op::Lt),
        ];
        for (sym, op) in ops {
            if let Some(i) = token.find(sym) {
                let key = &token[..i];
                let val = &token[i + sym.len()..];
                if key.is_empty() {
                    return None;
                }
                return Some(Filter {
                    paths: key
                        .split('|')
                        .map(|p| p.split('.').map(str::to_owned).collect())
                        .collect(),
                    op: *op,
                    values: val.split(',').map(str::to_owned).collect(),
                });
            }
        }
        None
    }

    pub fn matches(&self, json: &Json) -> bool {
        // First present (non-null) fallback path wins; if none present, the
        // field counts as missing.
        let field = self.paths.iter().find_map(|path| {
            let mut cur = json;
            for seg in path {
                cur = match seg.parse::<usize>() {
                    Ok(i) => cur.get_i(i),
                    Err(_) => cur.get(seg),
                };
            }
            cur.as_str().or_else(|| cur.as_value())
        });
        let Some(field) = field else {
            // missing field: only !=/!~ can match
            return matches!(self.op, Op::Ne | Op::NotContains);
        };
        match self.op {
            Op::Eq => self.values.iter().any(|v| v == field),
            Op::Ne => self.values.iter().all(|v| v != field),
            Op::Contains => self.values.iter().any(|v| field.contains(v.as_str())),
            Op::NotContains => self.values.iter().all(|v| !field.contains(v.as_str())),
            Op::Gt | Op::Lt | Op::Ge | Op::Le => {
                let Some(f) = parse_number(field) else { return false };
                self.values.iter().any(|v| {
                    parse_number(v).is_some_and(|n| match self.op {
                        Op::Gt => f > n,
                        Op::Lt => f < n,
                        Op::Ge => f >= n,
                        Op::Le => f <= n,
                        _ => false,
                    })
                })
            }
        }
    }
}

/// All filters must match (AND).
pub fn matches_all(filters: &[Filter], json: &Json) -> bool {
    filters.iter().all(|f| f.matches(json))
}

/// Parse a number from a field value for ordering/aggregation, tolerating a
/// trailing unit like the ` ms` in `"6.193 ms"` or a `%`/`s` suffix that logs
/// often attach. Strict `f64` parsing is tried first; otherwise the leading
/// numeric run (optional sign, digits, decimal, exponent) is taken and parsed,
/// so `"6.193 ms"` -> 6.193, `"200ms"` -> 200, and `"ok"` -> None. This lets
/// `n>500` filters and `stats` work on human-formatted numeric fields.
pub fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if let Ok(n) = s.parse::<f64>() {
        return Some(n);
    }
    let end = s
        .find(|c: char| !matches!(c, '0'..='9' | '.' | '-' | '+' | 'e' | 'E'))
        .unwrap_or(s.len());
    let head = s[..end].trim_end_matches(['.', '-', '+', 'e', 'E']);
    head.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_json;

    fn matches(token: &str, json: &str) -> bool {
        let f = Filter::parse(token).expect("token has an operator");
        f.matches(&parse_json(json).unwrap())
    }

    #[test]
    fn parse_returns_none_without_operator() {
        assert!(Filter::parse("level").is_none());
        assert!(Filter::parse("ts").is_none());
    }

    #[test]
    fn parse_returns_none_with_empty_key() {
        assert!(Filter::parse("=error").is_none());
    }

    #[test]
    fn two_char_ops_win_over_one_char() {
        // `!=`, `>=`, `<=`, `!~` must be detected before `=`, `>`, `<`, `~`.
        let r = r#"{"level":"warn","n":5}"#;
        assert!(matches("level!=error", r));
        assert!(matches("n>=5", r));
        assert!(matches("n<=5", r));
        assert!(matches("level!~err", r));
    }

    #[test]
    fn eq_and_ne_are_string_compares() {
        let r = r#"{"level":"error"}"#;
        assert!(matches("level=error", r));
        assert!(!matches("level=warn", r));
        assert!(matches("level!=warn", r));
        assert!(!matches("level!=error", r));
    }

    #[test]
    fn comma_value_is_or_within_a_field() {
        let r = r#"{"level":"warn"}"#;
        assert!(matches("level=error,warn", r));
        assert!(!matches("level=error,fatal", r));
        // !=/!~ require ALL listed values to differ
        assert!(!matches("level!=error,warn", r));
        assert!(matches("level!=error,fatal", r));
    }

    #[test]
    fn numeric_comparisons() {
        let r = r#"{"latency_ms":510}"#;
        assert!(matches("latency_ms>500", r));
        assert!(!matches("latency_ms<500", r));
        assert!(matches("latency_ms>=510", r));
        assert!(matches("latency_ms<=510", r));
        // non-numeric field never satisfies an ordering op
        assert!(!matches("latency_ms>x", r#"{"latency_ms":"abc"}"#));
        // a number with a unit suffix still compares numerically
        assert!(matches("latency>500", r#"{"latency":"510 ms"}"#));
        assert!(!matches("latency>500", r#"{"latency":"6.193 ms"}"#));
    }

    #[test]
    fn parse_number_tolerates_units() {
        assert_eq!(parse_number("510"), Some(510.0));
        assert_eq!(parse_number("6.193 ms"), Some(6.193));
        assert_eq!(parse_number("200ms"), Some(200.0));
        assert_eq!(parse_number("-3.5 s"), Some(-3.5));
        assert_eq!(parse_number("1.5e3 units"), Some(1500.0));
        assert_eq!(parse_number("  42  "), Some(42.0));
        assert_eq!(parse_number("ok"), None);
        assert_eq!(parse_number("$5"), None);
        assert_eq!(parse_number(""), None);
    }

    #[test]
    fn contains_and_not_contains() {
        let r = r#"{"msg":"db timeout"}"#;
        assert!(matches("msg~timeout", r));
        assert!(!matches("msg~connect", r));
        assert!(matches("msg!~connect", r));
        assert!(!matches("msg!~timeout", r));
    }

    #[test]
    fn nested_paths_resolve_by_dot() {
        let r = r#"{"data":{"user":{"id":3175}}}"#;
        assert!(matches("data.user.id=3175", r));
        assert!(matches("data.user.id>3000", r));
    }

    #[test]
    fn missing_field_only_matches_negations() {
        let r = r#"{"level":"error"}"#;
        assert!(matches("absent!=x", r));
        assert!(matches("absent!~x", r));
        assert!(!matches("absent=x", r));
        assert!(!matches("absent~x", r));
        assert!(!matches("absent>1", r));
    }

    #[test]
    fn fallback_fields_first_present_wins() {
        // `a|b|c=x` matches whichever of those keys is present.
        assert!(matches("lvl|level|severity=error", r#"{"lvl":"error"}"#));
        assert!(matches("lvl|level|severity=error", r#"{"severity":"error"}"#));
        assert!(matches("lvl|level|severity=error", r#"{"level":"error"}"#));
        assert!(!matches("lvl|level|severity=error", r#"{"level":"warn"}"#));
        // none present -> missing -> only !=/!~ match
        assert!(!matches("lvl|level|severity=error", r#"{"other":"error"}"#));
        assert!(matches("lvl|level|severity!=error", r#"{"other":"x"}"#));
    }

    #[test]
    fn fallback_key_roundtrips_for_override_dedup() {
        let f = Filter::parse("lvl|level|severity=error").unwrap();
        assert_eq!(f.key(), "lvl|level|severity");
        let nested = Filter::parse("data.user.id=1").unwrap();
        assert_eq!(nested.key(), "data.user.id");
    }

    #[test]
    fn matches_all_is_and_across_filters() {
        let json = parse_json(r#"{"level":"error","user":"alice"}"#).unwrap();
        let fs = [
            Filter::parse("level=error").unwrap(),
            Filter::parse("user=alice").unwrap(),
        ];
        assert!(matches_all(&fs, &json));
        let fs2 = [
            Filter::parse("level=error").unwrap(),
            Filter::parse("user=bob").unwrap(),
        ];
        assert!(!matches_all(&fs2, &json));
    }
}
