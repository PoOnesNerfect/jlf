use std::fmt::{self, Write};

use color_eyre::eyre::{eyre, Result};

pub fn expanded_format(format: &str, variables: &[(String, String)]) -> String {
    ExpandedFormat(format, variables).to_string()
}

#[inline]
pub fn get_variable<'a>(variables: &'a [(String, String)], key: &str) -> Result<&'a str> {
    variables
        .iter()
        .find_map(|(k, v)| (k == key).then_some(v.as_str()))
        .ok_or_else(|| eyre!("Variable doesn't exist: {key}"))
}

pub struct ExpandedFormat<'a>(pub &'a str, pub &'a [(String, String)]);

impl fmt::Display for ExpandedFormat<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(format_string, variables) = self;

        write_input(f, format_string, variables)?;

        Ok(())
    }
}

#[inline]
fn write_input(
    f: &mut fmt::Formatter<'_>,
    input: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    let mut chunks = input.split('\\');

    if let Some(chunk) = chunks.next() {
        write_chunk(f, chunk, variables)?;
    }

    for chunk in chunks {
        if !chunk.is_empty() {
            let (escaped, rest) = chunk.split_at(1);
            f.write_fmt(format_args!("\\{escaped}"))?;
            if !rest.is_empty() {
                write_chunk(f, rest, variables)?;
            }
        } else {
            f.write_str("\\\\")?;
        }
    }

    Ok(())
}

fn write_chunk(
    f: &mut fmt::Formatter<'_>,
    chunk: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    let mut parts = chunk.split('{');

    if let Some(part) = parts.next() {
        if !part.is_empty() {
            f.write_str(part)?;
        }
    }

    for part in parts {
        if let Some(end) = part.find('}') {
            let content = &part[..end];

            // '&' means a variable and needs to be expanded
            if content.starts_with('&') {
                write_variable(f, content, variables)?;
            } else if let Some(content) = content.strip_prefix('#') {
                write_cond(f, content, variables)?;
            } else if let Some(content) = content.strip_prefix(':') {
                write_cond_else(f, content, variables)?;
            } else if let Some(content) = content.strip_prefix('/') {
                f.write_fmt(format_args!("{{/{content}}}"))?;
            } else {
                write_arg(f, content, variables)?;
            }

            if let Some(literal) = &part.get(end + 1..) {
                if !literal.is_empty() {
                    f.write_str(literal)?;
                }
            }
        } else {
            panic!("Missing closing brace");
        }
    }

    Ok(())
}

fn write_variable(
    f: &mut fmt::Formatter<'_>,
    content: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    // if there's a formatting like `{&var:dimmed}`, it must be a field
    if let Some((content, style)) = content.split_once(':') {
        let mut e = String::new();
        let written = write_field(&mut e, content, variables)?;
        if written {
            f.write_char('{')?;
            f.write_str(&e)?;
            f.write_fmt(format_args!(":{style}}}"))?;
        }
    } else if content.contains('|') {
        let mut e = String::new();
        let written = write_field(&mut e, content, variables)?;
        if written {
            f.write_char('{')?;
            f.write_str(&e)?;
            f.write_char('}')?;
        }
    } else {
        let value = get_variable(variables, &content[1..]).unwrap();
        write_input(f, value, variables)?;
    }

    Ok(())
}

fn write_cond(
    f: &mut fmt::Formatter<'_>,
    content: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    // '#' means param is a conditional
    let (content, cond) = if let Some(content) = content.strip_prefix("key ") {
        (content, "key")
    } else if let Some(content) = content.strip_prefix("if ") {
        (content, "if")
    } else if let Some(content) = content.strip_prefix("config ") {
        (content, "config")
    } else {
        panic!("Unsupported conditional #{content}");
    };

    f.write_fmt(format_args!("{{#{cond} "))?;
    write_field(f, content, variables)?;
    f.write_char('}')?;

    Ok(())
}

fn write_cond_else(
    f: &mut fmt::Formatter<'_>,
    content: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    // ':' means `else` of conditional
    if content == "else" {
        f.write_str("{:else}")?;
    } else if let Some(content) = content.strip_prefix("else key ") {
        f.write_fmt(format_args!("{{:else key "))?;
        write_field(f, content, variables)?;
        f.write_char('}')?;
    } else if let Some(content) = content.strip_prefix("else if ") {
        f.write_fmt(format_args!("{{:else if "))?;
        write_field(f, content, variables)?;
        f.write_char('}')?;
    } else {
        panic!("Unsupported conditional :{content}");
    }

    Ok(())
}

fn write_arg(
    f: &mut fmt::Formatter<'_>,
    content: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    let (content, style) = content
        .split_once(':')
        .map(|(c, s)| (c, Some(s)))
        .unwrap_or((content, None));

    let mut e = String::new();
    write_field(&mut e, content, variables)?;

    if !e.is_empty() {
        f.write_char('{')?;
        f.write_str(&e)?;
        if let Some(style) = style {
            f.write_fmt(format_args!(":{style}"))?;
        }
        f.write_char('}')?;
    }

    Ok(())
}

// Tries to write the parsed field.
// In case where parsed fields were empty strings,
// it returns `false`.
fn write_field(
    f: &mut impl fmt::Write,
    content: &str,
    variables: &[(String, String)],
) -> Result<bool, fmt::Error> {
    let mut written = false;
    let mut prev_written = false;

    for field in content.split('|') {
        if prev_written {
            f.write_char('|')?;
        }

        if let Some(key) = field.strip_prefix('&') {
            let val = get_variable_field(variables, key);
            if !val.is_empty() {
                prev_written = write_field(f, val, variables)?;
                written |= prev_written;
            } else {
                prev_written = false;
            }
        } else if !field.is_empty() {
            f.write_str(field)?;
            prev_written = true;
            written = true;
        } else {
            prev_written = false;
            written |= prev_written;
        }
    }

    Ok(written)
}

fn get_variable_field<'a>(variables: &'a [(String, String)], key: &str) -> &'a str {
    let var = get_variable(variables, key).unwrap();
    if var.is_empty() {
        return var;
    }

    let ret = var
        .strip_prefix('{')
        .and_then(|e| e.strip_suffix('}'))
        .ok_or_else(|| eyre!("variable ({key}) expected to be a field, but found plain text: {var}\nMake sure to wrap the field with braces '{{{var}}}'"))
        .unwrap();

    ret
}
