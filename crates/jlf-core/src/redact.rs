use crate::Json;

/// Replace the value of any field matching a pattern with "***". Patterns:
/// a bare key (`password`) or `*.key` match that key at any depth; a dotted path
/// (`fields.message`) matches that rooted path (array indices are transparent, so
/// `spans.method` redacts `method` in every element of `spans`). Applied
/// recursively.
pub fn redact(json: &mut Json, patterns: &[String]) {
    redact_at(json, "", patterns);
}

fn redact_at(json: &mut Json, path: &str, patterns: &[String]) {
    match json {
        Json::Object(obj) => {
            for (k, v) in obj.iter_mut() {
                let child = if path.is_empty() {
                    (*k).to_owned()
                } else {
                    format!("{path}.{k}")
                };
                if patterns.iter().any(|p| matches(p, k, &child)) {
                    *v = Json::String("***");
                } else {
                    redact_at(v, &child, patterns);
                }
            }
        }
        Json::Array(arr) => {
            // Arrays are transparent: elements keep the array's path, so a dotted
            // pattern matches inside every element without needing an index.
            for v in arr.iter_mut() {
                redact_at(v, path, patterns);
            }
        }
        _ => {}
    }
}

fn matches(pattern: &str, key: &str, path: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix("*.") {
        key == suffix
    } else if pattern.contains('.') {
        path == pattern
    } else {
        pattern == key
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

    #[test]
    fn dotted_path_redacts_the_rooted_field() {
        let out = redacted(
            &["fields.message"],
            r#"{"message":"top","fields":{"message":"nested","status":200}}"#,
        );
        // only the nested fields.message is masked, not the top-level message
        assert!(out.contains(r#""message":"top""#));
        assert!(out.contains(r#""message":"***""#));
        assert!(out.contains(r#""status":200"#));
    }

    #[test]
    fn dotted_path_is_transparent_over_arrays() {
        let out = redacted(
            &["spans.method"],
            r#"{"spans":[{"method":"GET"},{"method":"POST"}]}"#,
        );
        assert_eq!(out.matches(r#""method":"***""#).count(), 2);
    }

    #[test]
    fn bare_key_still_matches_at_any_depth() {
        let out = redacted(&["message"], r#"{"fields":{"message":"x"}}"#);
        assert!(out.contains(r#""message":"***""#));
    }
}
