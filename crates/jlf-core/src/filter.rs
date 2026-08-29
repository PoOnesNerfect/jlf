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
            Op::Contains => {
                self.values.iter().any(|v| field.contains(v.as_str()))
            }
            Op::NotContains => {
                self.values.iter().all(|v| !field.contains(v.as_str()))
            }
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
/// first (via [`parse_number`], so units are tolerated); if both aren't
/// numeric, falls back to timestamps (via [`parse_datetime`]), so
/// `ts>2026-07-11T15:00Z` compares chronologically. Returns `None` when the two
/// values aren't comparable in either domain, which never satisfies an ordering
/// op.
fn order_compare(field: &str, value: &str) -> Option<std::cmp::Ordering> {
    if let (Some(a), Some(b)) = (parse_number(field), parse_number(value)) {
        return a.partial_cmp(&b);
    }
    match (parse_datetime(field), parse_datetime(value)) {
        (Some(a), Some(b)) => Some(a.cmp(&b)),
        _ => None,
    }
}

/// Parse a timestamp to nanoseconds since the Unix epoch, for chronological
/// ordering of `>`/`<` filters. Covers the common log formats without a
/// datetime dependency:
///
/// - ISO 8601 / RFC 3339: `2026-07-11T15:11:48.968666Z`, `2026-07-11 15:11:48`,
///   `2026-07-11T15:11:48+02:00`, `2026/07/11 15:04:05`, `2026-07-11` (date
///   only)
/// - log4j comma fraction: `2026-07-11 15:11:48,123`
/// - RFC 2822 / HTTP-date: `Wed, 21 Oct 2015 07:28:00 GMT`, `Tue, 01 Jul 2003
///   10:52:37 +0200`
/// - Apache common-log: `10/Oct/2000:13:55:36 -0700`
/// - month names: `21 Oct 2015 07:28:00`, `Oct 21, 2015 07:28:00`,
///   `11-Jul-2026`
/// - syslog (no year): `Oct 11 15:11:48` — ordered within a shared sentinel
///   year
///
/// Missing lower components default to zero, so a partial bound like
/// `2026-07-11` or `2026-07-11T15:11` works. Timezones understood: `Z`, `UTC`,
/// `GMT`, and numeric `±HH:MM` / `±HHMM` / `±HH` (UTC assumed when absent).
/// Named zone abbreviations (EST, PST, …) are ambiguous and not supported.
/// Returns `None` for anything that isn't a recognized timestamp.
pub fn parse_datetime(s: &str) -> Option<i128> {
    let s = strip_weekday(s.trim());
    parse_iso(s)
        .or_else(|| parse_clf(s))
        .or_else(|| parse_named(s))
        .or_else(|| parse_dmy_dash(s))
        .or_else(|| parse_syslog(s))
}

/// Days from civil, seconds-of-day, and timezone combined into epoch nanos.
fn combine(
    year: i64,
    month: i64,
    day: i64,
    time_nanos: i128,
    tz_subtract: i128,
) -> i128 {
    days_from_civil(year, month, day) as i128 * NANOS_PER_DAY + time_nanos
        - tz_subtract
}

/// ISO 8601 / RFC 3339: `YYYY[-/]MM[-/]DD` optionally followed by a `T`/space
/// and `HH[:MM[:SS[.,frac]]]` and a timezone (glued or space-separated).
fn parse_iso(s: &str) -> Option<i128> {
    let (date, rest) = match s.split_once(['T', ' ']) {
        Some((d, r)) => (d, Some(r)),
        None => (s, None),
    };
    let sep = if date.contains('-') {
        '-'
    } else if date.contains('/') {
        '/'
    } else {
        return None;
    };
    let mut d = date.split(sep);
    let year = int_field(d.next()?, 0, 9999)?;
    let month = int_field(d.next()?, 1, 12)?;
    let day = int_field(d.next()?, 1, 31)?;
    if d.next().is_some() {
        return None;
    }
    let (time_nanos, tz) = match rest {
        Some(rest) => parse_time_and_tz(rest)?,
        None => (0, 0),
    };
    Some(combine(year, month, day, time_nanos, tz))
}

/// Apache common-log: `DD/Mon/YYYY:HH:MM:SS` with an optional ` ±HHMM` offset.
fn parse_clf(s: &str) -> Option<i128> {
    let (dt, tz) = match s.split_once(' ') {
        Some((a, b)) => (a, parse_tz(b)?),
        None => (s, 0),
    };
    let (date, time) = dt.split_once(':')?;
    let mut d = date.split('/');
    let day = int_field(d.next()?, 1, 31)?;
    let month = month_name(d.next()?)?;
    let year = int_field(d.next()?, 0, 9999)?;
    if d.next().is_some() {
        return None;
    }
    Some(combine(year, month, day, parse_time(time)?, tz))
}

/// Month-name forms: `DD Mon YYYY …` or `Mon DD[,] YYYY …`, with an optional
/// trailing `HH:MM:SS` time and timezone token.
fn parse_named(s: &str) -> Option<i128> {
    let t: Vec<&str> = s.split_whitespace().collect();
    if t.len() < 3 {
        return None;
    }
    let (year, month, day) = if let Some(month) = month_name(t[0]) {
        (
            int_field(t[2], 0, 9999)?,
            month,
            int_field(t[1].trim_end_matches(','), 1, 31)?,
        )
    } else if let Some(month) = month_name(t[1]) {
        (int_field(t[2], 0, 9999)?, month, int_field(t[0], 1, 31)?)
    } else {
        return None;
    };
    let time_nanos = match t.get(3) {
        Some(time) => parse_time(time)?,
        None => 0,
    };
    let tz = match t.get(4) {
        Some(tz) => parse_tz(tz)?,
        None => 0,
    };
    if t.len() > 5 {
        return None;
    }
    Some(combine(year, month, day, time_nanos, tz))
}

/// Dash-separated month name: `DD-Mon-YYYY` with an optional ` HH:MM:SS` and
/// tz.
fn parse_dmy_dash(s: &str) -> Option<i128> {
    let mut it = s.split_whitespace();
    let date = it.next()?;
    let mut d = date.split('-');
    let day = int_field(d.next()?, 1, 31)?;
    let month = month_name(d.next()?)?;
    let year = int_field(d.next()?, 0, 9999)?;
    if d.next().is_some() {
        return None;
    }
    let time_nanos = match it.next() {
        Some(time) => parse_time(time)?,
        None => 0,
    };
    let tz = match it.next() {
        Some(tz) => parse_tz(tz)?,
        None => 0,
    };
    if it.next().is_some() {
        return None;
    }
    Some(combine(year, month, day, time_nanos, tz))
}

/// syslog / RFC 3164: `Mon DD HH:MM:SS` with no year. Placed in a fixed
/// sentinel year so such stamps order correctly among themselves (a leap year,
/// so `Feb 29` is valid); they aren't comparable to year-bearing timestamps.
fn parse_syslog(s: &str) -> Option<i128> {
    const SENTINEL_YEAR: i64 = 2000;
    let t: Vec<&str> = s.split_whitespace().collect();
    if t.len() != 3 {
        return None;
    }
    let month = month_name(t[0])?;
    let day = int_field(t[1], 1, 31)?;
    Some(combine(SENTINEL_YEAR, month, day, parse_time(t[2])?, 0))
}

/// Drop a leading weekday word (`Mon`/`Monday`, optionally comma-terminated) so
/// RFC 2822 / HTTP dates fall through to the month-name parser.
fn strip_weekday(s: &str) -> &str {
    let end = s.find([',', ' ']).unwrap_or(s.len());
    let word = s[..end].trim_end_matches(',');
    if word.len() >= 3 && word.is_ascii() && is_weekday(&word[..3]) {
        return s[end..].trim_start_matches([',', ' ']);
    }
    s
}

fn is_weekday(prefix3: &str) -> bool {
    let mut buf = [0u8; 3];
    buf.copy_from_slice(prefix3.as_bytes());
    buf.make_ascii_lowercase();
    matches!(
        &buf,
        b"mon" | b"tue" | b"wed" | b"thu" | b"fri" | b"sat" | b"sun"
    )
}

/// Map an English month name (full or 3-letter, any case, optional trailing
/// `.`) to its 1-based number.
fn month_name(s: &str) -> Option<i64> {
    let s = s.trim_end_matches('.');
    if s.len() < 3
        || !s.is_ascii()
        || !s.bytes().all(|b| b.is_ascii_alphabetic())
    {
        return None;
    }
    let mut buf = [0u8; 3];
    buf.copy_from_slice(&s.as_bytes()[..3]);
    buf.make_ascii_lowercase();
    let months: [&[u8; 3]; 12] = [
        b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep",
        b"oct", b"nov", b"dec",
    ];
    months.iter().position(|m| *m == &buf).map(|i| i as i64 + 1)
}

/// Parse `HH[:MM[:SS[.,frac]]]` into nanoseconds-of-day, tolerating a `.` or
/// `,` fractional separator (log4j uses the comma).
fn parse_time(time: &str) -> Option<i128> {
    let mut t = time.split(':');
    let hour = int_field(t.next()?, 0, 23)?;
    let minute = match t.next() {
        Some(m) => int_field(m, 0, 59)?,
        None => 0,
    };
    let (second, frac_nanos) = match t.next() {
        Some(sec) => {
            let (whole, frac) = sec.split_once(['.', ',']).unwrap_or((sec, ""));
            (int_field(whole, 0, 60)?, parse_fraction_nanos(frac)?)
        }
        None => (0, 0),
    };
    if t.next().is_some() {
        return None;
    }
    Some(
        (hour * 3600 + minute * 60 + second) as i128 * 1_000_000_000
            + frac_nanos,
    )
}

/// Split a time-of-day from its timezone (glued as in `15:11:48Z` / `…+02:00`,
/// or a separate token as in `15:11:48 GMT`) and parse both.
fn parse_time_and_tz(rest: &str) -> Option<(i128, i128)> {
    if let Some((time, tz)) = rest.split_once(' ') {
        return Some((parse_time(time)?, parse_tz(tz)?));
    }
    if let Some(time) = rest.strip_suffix(['Z', 'z']) {
        return Some((parse_time(time)?, 0));
    }
    // A `+`/`-` after the hour introduces a numeric offset glued to the time.
    if let Some(pos) = rest
        .get(1..)
        .and_then(|r| r.find(['+', '-']))
        .map(|i| i + 1)
    {
        return Some((parse_time(&rest[..pos])?, parse_tz(&rest[pos..])?));
    }
    Some((parse_time(rest)?, 0))
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

/// Convert the digits after a `.`/`,` in the seconds field to nanoseconds,
/// padding or truncating to 9 digits (`.5` -> 500_000_000). Empty is allowed
/// (0).
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

/// Nanoseconds to subtract from a local time to get UTC. `Z`/`UTC`/`GMT` is 0;
/// numeric `±HH:MM` / `±HHMM` / `±HH` shift accordingly (`+` is ahead of UTC).
fn parse_tz(tz: &str) -> Option<i128> {
    match tz {
        "Z" | "z" | "UTC" | "GMT" | "UT" => return Some(0),
        _ => {}
    }
    let (sign, hm) = tz.split_at(1);
    let sign = match sign {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let (h, m) = match hm.split_once(':') {
        Some((h, m)) => (h, m),
        None => match hm.len() {
            4 => (&hm[..2], &hm[2..]),
            2 => (hm, "00"),
            _ => return None,
        },
    };
    let hours = int_field(h, 0, 23)?;
    let minutes = int_field(m, 0, 59)?;
    Some(sign * (hours * 3600 + minutes * 60) as i128 * 1_000_000_000)
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
        assert_eq!(parse_datetime("2026-07-11T17:11:48+02:00"), Some(base));
        // ordering across days and the epoch reference
        assert!(parse_datetime("2026-07-12") > parse_datetime("2026-07-11"));
        assert_eq!(parse_datetime("1970-01-01T00:00:00Z"), Some(0));
        // junk is rejected
        assert_eq!(parse_datetime("2026-13-11"), None);
        assert_eq!(parse_datetime("nope"), None);
        assert_eq!(parse_datetime("2026-07-11T25:00:00Z"), None);
    }

    #[test]
    fn parse_datetime_covers_common_formats() {
        let base = parse_datetime("2015-10-21T07:28:00Z").unwrap();
        // slash-separated ISO date
        assert_eq!(parse_datetime("2015/10/21 07:28:00Z"), Some(base));
        // log4j comma fraction
        assert_eq!(
            parse_datetime("2015-10-21 07:28:00,250"),
            Some(base + 250_000_000)
        );
        // RFC 2822 / HTTP-date (leading weekday stripped) with GMT and offsets
        assert_eq!(parse_datetime("Wed, 21 Oct 2015 07:28:00 GMT"), Some(base));
        assert_eq!(
            parse_datetime("Wed, 21 Oct 2015 09:28:00 +02:00"),
            Some(base)
        );
        assert_eq!(
            parse_datetime("Wed, 21 Oct 2015 05:28:00 -0200"),
            Some(base)
        );
        // month-name forms in both orders
        assert_eq!(parse_datetime("21 Oct 2015 07:28:00"), Some(base));
        assert_eq!(parse_datetime("Oct 21, 2015 07:28:00"), Some(base));
        assert_eq!(parse_datetime("October 21, 2015 07:28:00 UTC"), Some(base));
        // Apache common-log
        assert_eq!(parse_datetime("21/Oct/2015:07:28:00 +0000"), Some(base));
        assert_eq!(parse_datetime("21/Oct/2015:00:28:00 -0700"), Some(base));
        // dash-separated month name
        assert_eq!(parse_datetime("21-Oct-2015 07:28:00"), Some(base));
        // date-only and partial-time bounds
        assert_eq!(
            parse_datetime("2015-10-21"),
            parse_datetime("2015-10-21T00:00:00Z")
        );
        assert_eq!(
            parse_datetime("2015-10-21T07:28"),
            parse_datetime("2015-10-21T07:28:00Z")
        );
        // syslog (no year) orders within itself; unknown named zones rejected
        assert!(
            parse_datetime("Oct 21 07:28:00")
                < parse_datetime("Oct 21 07:29:00")
        );
        assert!(
            parse_datetime("Feb 09 00:00:00")
                < parse_datetime("Dec 09 00:00:00")
        );
        assert_eq!(parse_datetime("2015-10-21T07:28:00 EST"), None);
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
        assert!(matches(
            "lvl|level|severity=error",
            r#"{"severity":"error"}"#
        ));
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
