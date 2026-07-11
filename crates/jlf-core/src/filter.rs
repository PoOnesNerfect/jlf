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
            Op::Gt | Op::Lt | Op::Ge | Op::Le => self.values.iter().any(|v| {
                order_compare(field, v).is_some_and(|ord| match self.op {
                    Op::Gt => ord.is_gt(),
                    Op::Lt => ord.is_lt(),
                    Op::Ge => ord.is_ge(),
                    Op::Le => ord.is_le(),
                    _ => false,
                })
            }),
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

/// Order two field values for a `>`/`<`/`>=`/`<=` comparison. Tries numbers
/// first (via [`parse_number`], so units are tolerated); if both aren't numeric,
/// falls back to timestamps (via [`parse_datetime`]), so `ts>2026-07-11T15:00Z`
/// compares chronologically. Returns `None` when the two values aren't
/// comparable in either domain, which never satisfies an ordering op.
fn order_compare(field: &str, value: &str) -> Option<std::cmp::Ordering> {
    if let (Some(a), Some(b)) = (parse_number(field), parse_number(value)) {
        return a.partial_cmp(&b);
    }
    match (parse_datetime(field), parse_datetime(value)) {
        (Some(a), Some(b)) => Some(a.cmp(&b)),
        _ => None,
    }
}

/// Parse an RFC 3339 / ISO 8601 timestamp to nanoseconds since the Unix epoch,
/// for chronological ordering. Accepts `YYYY-MM-DD` optionally followed by a
/// `T`/space and `HH:MM[:SS[.fraction]]`, and an optional `Z` or `±HH:MM`
/// timezone (UTC assumed when absent). Missing lower components default to zero,
/// so a filter value like `2026-07-11` or `2026-07-11T15:11` works as a bound.
/// Returns `None` for anything that isn't a well-formed timestamp.
pub fn parse_datetime(s: &str) -> Option<i128> {
    let s = s.trim();
    let (date, rest) = match s.split_once(['T', ' ']) {
        Some((d, r)) => (d, Some(r)),
        None => (s, None),
    };

    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = int_field(d.next()?, 1, 12)?;
    let day: i64 = int_field(d.next()?, 1, 31)?;
    if d.next().is_some() {
        return None;
    }

    let mut nanos = days_from_civil(year, month, day) as i128 * NANOS_PER_DAY;

    if let Some(rest) = rest {
        let (time, tz) = split_timezone(rest);
        let mut t = time.split(':');
        let hour = int_field(t.next()?, 0, 23)?;
        let minute = match t.next() {
            Some(m) => int_field(m, 0, 59)?,
            None => 0,
        };
        let (second, frac_nanos) = match t.next() {
            Some(sec) => {
                let (whole, frac) = sec.split_once('.').unwrap_or((sec, ""));
                (int_field(whole, 0, 60)?, parse_fraction_nanos(frac)?)
            }
            None => (0, 0),
        };
        if t.next().is_some() {
            return None;
        }
        nanos += (hour * 3600 + minute * 60 + second) as i128 * 1_000_000_000 + frac_nanos;
        nanos -= timezone_offset_nanos(tz)?;
    }
    Some(nanos)
}

const NANOS_PER_DAY: i128 = 86_400 * 1_000_000_000;

/// Parse a zero-padded integer field and bounds-check it, rejecting signs and
/// non-digits so a stray token can't masquerade as part of a timestamp.
fn int_field(s: &str, lo: i64, hi: i64) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i64 = s.parse().ok()?;
    (lo..=hi).contains(&n).then_some(n)
}

/// Convert the digits after a `.` in the seconds field to nanoseconds, padding
/// or truncating to 9 digits (`.5` -> 500_000_000). Empty is allowed (0).
fn parse_fraction_nanos(frac: &str) -> Option<i128> {
    if frac.is_empty() {
        return Some(0);
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut digits = frac.as_bytes().to_vec();
    digits.resize(9, b'0');
    std::str::from_utf8(&digits[..9]).ok()?.parse().ok()
}

/// Split the time-of-day from a trailing timezone (`Z`, `+HH:MM`, or `-HH:MM`).
fn split_timezone(rest: &str) -> (&str, Option<&str>) {
    if let Some(t) = rest.strip_suffix(['Z', 'z']) {
        return (t, Some("Z"));
    }
    match rest.rfind(['+', '-']) {
        Some(pos) => (&rest[..pos], Some(&rest[pos..])),
        None => (rest, None),
    }
}

/// Nanoseconds to subtract to convert a local time to UTC. `None`/`Z` is 0;
/// `+HH:MM` shifts UTC ahead of local (subtract), `-HH:MM` behind (add).
fn timezone_offset_nanos(tz: Option<&str>) -> Option<i128> {
    let tz = match tz {
        None | Some("Z") => return Some(0),
        Some(tz) => tz,
    };
    let (sign, hm) = tz.split_at(1);
    let (h, m) = hm.split_once(':')?;
    let hours = int_field(h, 0, 23)?;
    let minutes = int_field(m, 0, 59)?;
    let secs = hours * 3600 + minutes * 60;
    let nanos = secs as i128 * 1_000_000_000;
    match sign {
        "+" => Some(nanos),
        "-" => Some(-nanos),
        _ => None,
    }
}

/// Days from the Unix epoch (1970-01-01) to a civil date, by Howard Hinnant's
/// algorithm. Valid for any proleptic-Gregorian date; the month/day are assumed
/// already range-checked by the caller.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
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
    fn timestamp_comparisons() {
        let r = r#"{"ts":"2026-07-11T15:11:48.968666Z"}"#;
        assert!(matches("ts>2026-07-11T15:00:00Z", r));
        assert!(matches("ts<2026-07-11T16:00:00Z", r));
        assert!(!matches("ts>2026-07-11T16:00:00Z", r));
        // partial bounds default missing components to zero
        assert!(matches("ts>2026-07-11", r));
        assert!(matches("ts<2026-07-12", r));
        assert!(matches("ts>2026-07-11T15:11", r));
        // an unparseable bound never matches
        assert!(!matches("ts>not-a-date", r));
    }

    #[test]
    fn parse_datetime_normalizes_forms() {
        let base = parse_datetime("2026-07-11T15:11:48Z").unwrap();
        // fractional seconds add nanoseconds
        assert_eq!(
            parse_datetime("2026-07-11T15:11:48.5Z").unwrap(),
            base + 500_000_000
        );
        // 'Z', space separator, and an explicit +00:00 all agree
        assert_eq!(parse_datetime("2026-07-11 15:11:48Z"), Some(base));
        assert_eq!(parse_datetime("2026-07-11T15:11:48+00:00"), Some(base));
        // a +02:00 local time is two hours earlier in UTC
        assert_eq!(
            parse_datetime("2026-07-11T17:11:48+02:00"),
            Some(base)
        );
        // ordering across days and the epoch reference
        assert!(parse_datetime("2026-07-12") > parse_datetime("2026-07-11"));
        assert_eq!(parse_datetime("1970-01-01T00:00:00Z"), Some(0));
        // junk is rejected
        assert_eq!(parse_datetime("2026-13-11"), None);
        assert_eq!(parse_datetime("nope"), None);
        assert_eq!(parse_datetime("2026-07-11T25:00:00Z"), None);
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
