use owo_colors::{
    colors::{Blue, BrightWhite, Green, White},
    OwoColorize, Style,
};
use std::fmt;

use crate::{format::KeyType, Json};

use super::exclude_fields::StyledJsonExcludeFields;

pub struct StyledJson<'a> {
    pub json: &'a Json<'a>,
    pub indent: usize,
    pub styles: Option<MarkupStyles>,
}

impl<'a> StyledJson<'a> {
    pub fn indented(self, indent: usize) -> Self {
        Self { indent, ..self }
    }

    pub fn styled(self, styles: MarkupStyles) -> Self {
        Self {
            styles: Some(styles),
            ..self
        }
    }

    pub fn exclude_fields<T>(self, exclude: T) -> StyledJsonExcludeFields<'a, T>
    where
        T: Iterator<Item = &'a [KeyType]> + Clone,
    {
        StyledJsonExcludeFields {
            json: self.json,
            indent: self.indent,
            styles: self.styles,
            exclude,
        }
    }
}

impl<'a> fmt::Display for StyledJson<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.json.fmt_compact(f, &self.styles)
    }
}

impl fmt::Debug for StyledJson<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        self.json.fmt_pretty(f, self.indent, &self.styles)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MarkupStyles {
    pub key: Style,
    pub value: Style,
    pub str: Style,
    pub syntax: Style,
}

impl Default for MarkupStyles {
    fn default() -> Self {
        Self {
            key: Style::new().fg::<Blue>(),
            value: Style::new().fg::<BrightWhite>(),
            str: Style::new().fg::<Green>(),
            syntax: Style::new().fg::<White>(),
        }
    }
}

impl<'a> fmt::Display for Json<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.fmt_compact(f, &None)
    }
}

impl<'a> fmt::Debug for Json<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.fmt_pretty(f, 0, &None)
    }
}

impl Json<'_> {
    pub fn fmt_compact(
        &self,
        f: &mut fmt::Formatter<'_>,
        styles: &Option<MarkupStyles>,
    ) -> Result<(), fmt::Error> {
        match self {
            Json::Object(obj) => {
                write_syntax(f, "{", styles)?;

                for (key, value) in obj.iter().take(obj.0.len().saturating_sub(1)) {
                    if !value.is_null() {
                        write_key(f, key, styles)?;
                        write_syntax(f, ":", styles)?;
                        value.fmt_compact(f, styles)?;
                        write_syntax(f, ",", styles)?;
                    }
                }

                if let Some((key, value)) = obj.0.last() {
                    if !value.is_null() {
                        write_key(f, key, styles)?;
                        write_syntax(f, ":", styles)?;
                        value.fmt_compact(f, styles)?;
                    }
                }

                write_syntax(f, "}", styles)
            }
            Json::Array(arr) => {
                write_syntax(f, "[", styles)?;

                for value in arr.iter().take(arr.len().saturating_sub(1)) {
                    if !value.is_null() {
                        value.fmt_compact(f, styles)?;
                        write_syntax(f, ",", styles)?;
                    }
                }
                if let Some(value) = arr.last() {
                    if !value.is_null() {
                        value.fmt_compact(f, styles)?;
                    }
                }

                write_syntax(f, "]", styles)
            }
            Json::String(v) => write_str(f, v, styles),
            Json::Value(v) => write_value(f, v, styles),
            Json::Null | Json::NullPrevObject(_) | Json::NullPrevArray(_) => {
                write_value(f, "null", styles)
            }
        }
    }

    pub fn fmt_pretty(
        &self,
        f: &mut fmt::Formatter<'_>,
        indent: usize,
        styles: &Option<MarkupStyles>,
    ) -> Result<(), fmt::Error> {
        match self {
            Json::Object(obj) => {
                if obj.is_empty() {
                    return write_syntax(f, "{}", styles);
                }

                let mut non_nulls = obj.iter().filter(|(_, v)| !v.is_null());
                let Some((key, value)) = non_nulls.next() else {
                    return write_syntax(f, "{}", styles);
                };

                write_syntax(f, "{", styles)?;
                write!(f, "\n{:indent$}", "", indent = (indent + 2))?;
                write_key(f, key, styles)?;
                write_syntax(f, ":", styles)?;
                write!(f, " ")?;
                value.fmt_pretty(f, indent + 2, styles)?;

                for (key, value) in non_nulls {
                    write_syntax(f, ",", styles)?;
                    write!(f, "\n{:indent$}", "", indent = (indent + 2))?;
                    write_key(f, key, styles)?;
                    write_syntax(f, ":", styles)?;
                    write!(f, " ")?;
                    value.fmt_pretty(f, indent + 2, styles)?;
                }

                write!(f, "\n{:indent$}", "", indent = indent)?;
                write_syntax(f, "}", styles)
            }
            Json::Array(arr) => {
                if arr.is_empty() {
                    return write_syntax(f, "[]", styles);
                }

                let mut non_nulls = arr.iter().filter(|v| !v.is_null());
                let Some(value) = non_nulls.next() else {
                    return write_syntax(f, "[]", styles);
                };

                write_syntax(f, "[", styles)?;
                write!(f, "\n{:indent$}", "", indent = (indent + 2))?;
                value.fmt_pretty(f, indent + 2, styles)?;

                for value in non_nulls {
                    write_syntax(f, ",", styles)?;
                    write!(f, "\n{:indent$}", "", indent = (indent + 2))?;
                    value.fmt_pretty(f, indent + 2, styles)?;
                }
                write!(f, "\n{:indent$}", "", indent = indent)?;
                write_syntax(f, "]", styles)
            }
            Json::String(v) => write_str(f, v, styles),
            Json::Value(v) => write_value(f, v, styles),
            Json::Null | Json::NullPrevObject(_) | Json::NullPrevArray(_) => {
                write_value(f, "null", styles)
            }
        }
    }
}

pub(super) fn write_key(
    f: &mut fmt::Formatter<'_>,
    key: &str,
    styles: &Option<MarkupStyles>,
) -> Result<(), fmt::Error> {
    if let Some(style) = styles {
        write!(f, "{}", format_args!("\"{key}\"").style(style.key))
    } else {
        write!(f, "\"{}\"", key)
    }
}

pub(super) fn write_value(
    f: &mut fmt::Formatter<'_>,
    value: &str,
    styles: &Option<MarkupStyles>,
) -> Result<(), fmt::Error> {
    if let Some(style) = styles {
        write!(f, "{}", value.style(style.value))
    } else {
        write!(f, "{}", value)
    }
}

pub(super) fn write_str(
    f: &mut fmt::Formatter<'_>,
    str: &str,
    styles: &Option<MarkupStyles>,
) -> Result<(), fmt::Error> {
    if let Some(style) = styles {
        write!(f, "{}", format_args!("\"{str}\"").style(style.str))
    } else {
        write!(f, "\"{}\"", str)
    }
}

pub(super) fn write_syntax(
    f: &mut fmt::Formatter<'_>,
    syntax: &str,
    styles: &Option<MarkupStyles>,
) -> Result<(), fmt::Error> {
    if let Some(style) = styles {
        write!(f, "{}", syntax.style(style.syntax))
    } else {
        write!(f, "{}", syntax)
    }
}
