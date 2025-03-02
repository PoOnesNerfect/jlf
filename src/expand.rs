use std::fmt::{self, Write};

use crate::format::parse::get_variable;

pub struct ExpandedFormat(pub String, pub Vec<(String, String)>);

impl fmt::Display for ExpandedFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(format_string, variables) = self;

        write_input(f, format_string, variables)?;

        Ok(())
    }
}

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
        let (escaped, rest) = chunk.split_at(1);
        f.write_fmt(format_args!("\\{escaped}"))?;
        write_chunk(f, rest, variables)?;
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
            if let Some(key) = content.strip_prefix('&') {
                write_variable(f, key, variables)?;
            } else if let Some(content) = content.strip_prefix('#') {
                write_cond(f, content, variables)?;
            } else if let Some(content) = content.strip_prefix(':') {
                write_cond_else(f, content, variables)?;
            } else if let Some(content) = content.strip_prefix('/') {
                f.write_fmt(format_args!("{{/{content}}}"))?;
            } else {
                write_arg(f, content, variables)?;
            }

            let literal = &part[end + 1..];
            if !literal.is_empty() {
                f.write_str(literal)?;
            }
        } else {
            panic!("Missing closing brace");
        }
    }

    Ok(())
}

fn write_variable(
    f: &mut fmt::Formatter<'_>,
    key: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    // if there's a formatting like `{&var:dimmed}`, crunch as field
    if let Some((key, style)) = key.split_once(':') {
        let value = get_variable(variables, key)
            .unwrap()
            .trim_start_matches('{')
            .trim_end_matches('}');

        f.write_char('{')?;
        write_field(f, value, variables)?;
        f.write_fmt(format_args!(":{style}}}"))?;
    } else {
        let value = get_variable(variables, key).unwrap();
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

    f.write_char('{')?;
    write_field(f, content, variables)?;
    if let Some(style) = style {
        f.write_fmt(format_args!(":{style}"))?;
    }
    f.write_char('}')?;

    Ok(())
}

fn write_field(
    f: &mut fmt::Formatter<'_>,
    content: &str,
    variables: &[(String, String)],
) -> fmt::Result {
    let mut iter = content.split('|');
    if let Some(field) = iter.next() {
        if let Some(key) = field.strip_prefix('&') {
            let val = get_variable(variables, key)
                .unwrap()
                .trim_start_matches('{')
                .trim_end_matches('}');
            write_field(f, val, variables)?;
        } else {
            f.write_str(field)?;
        }
    }

    for field in iter {
        f.write_char('|')?;
        if let Some(key) = field.strip_prefix('&') {
            let val = get_variable(variables, key)
                .unwrap()
                .trim_start_matches('{')
                .trim_end_matches('}');
            write_field(f, val, variables)?;
        } else {
            f.write_str(field)?;
        }
    }

    Ok(())
}
