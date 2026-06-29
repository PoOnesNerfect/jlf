use crate::Json;

/// Replace the value of any field whose key matches a pattern with "***".
/// Patterns: exact key (`password`) or `*.key` (any depth). Applied recursively.
pub fn redact(json: &mut Json, patterns: &[String]) {
    match json {
        Json::Object(obj) => {
            for (k, v) in obj.iter_mut() {
                if patterns.iter().any(|p| matches(p, k)) {
                    *v = Json::String("***");
                } else {
                    redact(v, patterns);
                }
            }
        }
        Json::Array(arr) => {
            for v in arr.iter_mut() {
                redact(v, patterns);
            }
        }
        _ => {}
    }
}

fn matches(pattern: &str, key: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(suffix) => key == suffix,
        None => pattern == key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_json;

    fn redacted(patterns: &[&str], input: &str) -> String {
        let owned: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
        let mut json = parse_json(input).unwrap();
        redact(&mut json, &owned);
        json.to_string()
    }

    #[test]
    fn exact_key_top_level() {
        let out = redacted(&["token"], r#"{"user":"alice","token":"abc"}"#);
        assert!(out.contains(r#""token":"***""#));
        assert!(out.contains(r#""user":"alice""#));
    }

    #[test]
    fn nested_key_is_redacted_at_any_depth() {
        let out = redacted(&["password"], r#"{"a":{"b":{"password":"hunter2"}}}"#);
        assert!(out.contains(r#""password":"***""#));
    }

    #[test]
    fn star_dot_pattern_matches_key_at_any_depth() {
        let out = redacted(&["*.email"], r#"{"u":{"email":"a@b.com"},"email":"c@d.com"}"#);
        assert_eq!(out.matches(r#""email":"***""#).count(), 2);
    }

    #[test]
    fn non_matching_keys_are_untouched() {
        let input = r#"{"user":"alice","n":5}"#;
        assert_eq!(redacted(&["token"], input), parse_json(input).unwrap().to_string());
    }

    #[test]
    fn redacts_inside_arrays() {
        let out = redacted(&["token"], r#"{"items":[{"token":"x"},{"token":"y"}]}"#);
        assert_eq!(out.matches(r#""token":"***""#).count(), 2);
    }
}
