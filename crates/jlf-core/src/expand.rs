use color_eyre::eyre::{eyre, Result};

/// Inline `${@name}` / `${?@name}` recipe includes in a `$`-DSL template,
/// expanding each to the named variable's template (recursively). Everything
/// else — fields, `$( … )` repetitions, `${if …}` directives — passes through
/// untouched for the formatter to parse.
pub fn expanded_format(format: &str, variables: &[(String, String)]) -> String {
    let mut out = String::new();
    expand_into(&mut out, format, variables, 0);
    out
}

#[inline]
pub fn get_variable<'a>(variables: &'a [(String, String)], key: &str) -> Result<&'a str> {
    variables
        .iter()
        .find_map(|(k, v)| (k == key).then_some(v.as_str()))
        .ok_or_else(|| eyre!("Variable doesn't exist: {key}"))
}

fn expand_into(out: &mut String, input: &str, variables: &[(String, String)], depth: usize) {
    if depth > 64 {
        out.push_str(input); // cycle guard
        return;
    }
    let b = input.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' {
            match b.get(i + 1) {
                Some(b'$') => {
                    out.push_str("$$");
                    i += 2;
                    continue;
                }
                Some(b'{') => {
                    if let Some(rel) = input[i + 2..].find('}') {
                        let content = &input[i + 2..i + 2 + rel];
                        let after = i + 2 + rel + 1;
                        let trimmed = content.trim();
                        if let Some(name) = trimmed.strip_prefix("?@") {
                            if let Ok(val) = get_variable(variables, name.trim()) {
                                let mut inner = String::new();
                                expand_into(&mut inner, val, variables, depth + 1);
                                out.push_str(&make_optional(&inner));
                            }
                            i = after;
                            continue;
                        } else if let Some(name) = trimmed.strip_prefix('@') {
                            if let Ok(val) = get_variable(variables, name.trim()) {
                                expand_into(out, val, variables, depth + 1);
                            }
                            i = after;
                            continue;
                        } else {
                            out.push_str(&input[i..after]);
                            i = after;
                            continue;
                        }
                    } else {
                        out.push_str(&input[i..]);
                        break;
                    }
                }
                _ => {
                    out.push('$');
                    i += 1;
                    continue;
                }
            }
        }
        // copy a run of non-`$` bytes (UTF-8 safe: `$` is ASCII)
        let start = i;
        while i < b.len() && b[i] != b'$' {
            i += 1;
        }
        out.push_str(&input[start..i]);
    }
}

/// Make an inlined single-field include optional: `${x}` -> `${?x}`,
/// `$x` -> `${?x}`. A multi-field expansion passes through unchanged.
fn make_optional(inner: &str) -> String {
    let t = inner.trim_end();
    let trailing = &inner[t.len()..];
    if let Some(rest) = t.strip_prefix("${") {
        if rest.ends_with('}') && !rest[..rest.len() - 1].contains("${") {
            return format!("${{?{rest}{trailing}");
        }
    }
    inner.to_owned()
}
