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
