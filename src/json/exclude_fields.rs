use super::styled::{write_key, write_str, write_syntax, write_value, MarkupStyles};
use crate::format::KeyType;
use crate::Json;
use std::fmt;

pub struct StyledJsonExcludeFields<'a, T>
where
    T: Iterator<Item = &'a [KeyType]> + Clone,
{
    pub json: &'a Json<'a>,
    pub indent: usize,
    pub styles: Option<MarkupStyles>,
    pub exclude: T,
}

impl<'a, T> fmt::Display for StyledJsonExcludeFields<'a, T>
where
    T: Iterator<Item = &'a [KeyType]> + Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        self.fmt_compact(f)
    }
}

impl<'a, T> fmt::Debug for StyledJsonExcludeFields<'a, T>
where
    T: Iterator<Item = &'a [KeyType]> + Clone,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        self.fmt_pretty(f)
    }
}

impl<'a, T> StyledJsonExcludeFields<'a, T>
where
    T: Iterator<Item = &'a [KeyType]> + Clone,
{
    pub fn fmt_compact(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let Self {
            json,
            indent: _,
            styles,
            exclude,
        } = self;
        match json {
            Json::Object(obj) => {
                write_syntax(f, "{", styles)?;

                let write_pair = |f: &mut fmt::Formatter<'_>, key: &str, val: &Json<'_>| {
                    write_key(f, key, styles)?;
                    write_syntax(f, ":", styles)?;

                    let mut val = val.exclude_fields(exclude.clone().filter_map(|e| {
                        if let Some(KeyType::Name(k)) = e.get(0) {
                            if k == key && e.len() > 1 {
                                return Some(&e[1..]);
                            }
                        }

                        None
                    }));
                    val.styles = *styles;
                    val.fmt_compact(f)?;

                    Ok::<_, fmt::Error>(())
                };

                // exclude if the field is in the exclude list
                let should_exclude = |key: &str| {
                    exclude.clone().any(|e| {
                        if let Some(KeyType::Name(k)) = e.get(0) {
                            k == key && e.len() == 1
                        } else {
                            false
                        }
                    })
                };

                for (key, value) in obj.iter().take(obj.0.len().saturating_sub(1)) {
                    if !value.is_null() && !should_exclude(key) {
                        write_pair(f, key, value)?;
                        write_syntax(f, ",", styles)?;
                    }
                }

                if let Some((key, value)) = obj.0.last() {
                    if !value.is_null() && !should_exclude(key) {
                        write_pair(f, key, value)?;
                    }
                }

                write_syntax(f, "}", styles)
            }
            Json::Array(arr) => {
                write_syntax(f, "[", styles)?;

                // exclude if the field is in the exclude list
                let should_exclude = |index: usize| {
                    exclude.clone().any(|e| {
                        if let Some(KeyType::Index(i)) = e.get(0) {
                            *i == index && e.len() == 1
                        } else {
                            false
                        }
                    })
                };

                for (index, value) in arr.iter().take(arr.len().saturating_sub(1)).enumerate() {
                    if !value.is_null() && !should_exclude(index) {
                        let mut val = value.exclude_fields(exclude.clone().filter_map(|e| {
                            if let Some(KeyType::Index(i)) = e.get(0) {
                                if *i == index && e.len() > 1 {
                                    return Some(&e[1..]);
                                }
                            }

                            None
                        }));
                        val.styles = *styles;
                        val.fmt_compact(f)?;
                        write_syntax(f, ",", styles)?;
                    }
                }
                if let Some(value) = arr.last() {
                    if !value.is_null() && !should_exclude(arr.len() - 1) {
                        let mut val = value.exclude_fields(exclude.clone().filter_map(|e| {
                            if let Some(KeyType::Index(i)) = e.get(0) {
                                if *i == arr.len() - 1 && e.len() > 1 {
                                    return Some(&e[1..]);
                                }
                            }

                            None
                        }));
                        val.styles = *styles;
                        val.fmt_compact(f)?;
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

    pub fn fmt_pretty(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let Self {
            json,
            indent,
            styles,
            exclude,
        } = self;

        match json {
            Json::Object(obj) => {
                if obj.is_empty() {
                    return write_syntax(f, "{}", styles);
                }

                // exclude if the field is in the exclude list
                let should_exclude = |key: &str| {
                    exclude.clone().any(|e| {
                        if let Some(KeyType::Name(k)) = e.get(0) {
                            k == key && e.len() == 1
                        } else {
                            false
                        }
                    })
                };

                let mut non_nulls = obj
                    .iter()
                    .filter(|(k, v)| !v.is_null() && !should_exclude(k));
                let Some((key, value)) = non_nulls.next() else {
                    return write_syntax(f, "{}", styles);
                };

                write_syntax(f, "{", styles)?;
                write!(f, "\n{:indent$}", "", indent = (indent + 2))?;
                write_key(f, key, styles)?;
                write_syntax(f, ":", styles)?;
                write!(f, " ")?;

                let mut value =
                    value
                        .indented(*indent + 2)
                        .exclude_fields(exclude.clone().filter_map(|e| {
                            if let Some(KeyType::Name(k)) = e.get(0) {
                                if k == key && e.len() > 1 {
                                    return Some(&e[1..]);
                                }
                            }

                            None
                        }));
                value.styles = *styles;
                value.fmt_pretty(f)?;

                for (key, value) in non_nulls {
                    write_syntax(f, ",", styles)?;
                    write!(f, "\n{:indent$}", "", indent = (indent + 2))?;
                    write_key(f, key, styles)?;
                    write_syntax(f, ":", styles)?;
                    write!(f, " ")?;
                    let mut value =
                        value
                            .indented(*indent + 2)
                            .exclude_fields(exclude.clone().filter_map(|e| {
                                if let Some(KeyType::Name(k)) = e.get(0) {
                                    if k == key && e.len() > 1 {
                                        return Some(&e[1..]);
                                    }
                                }

                                None
                            }));
                    value.styles = *styles;
                    value.fmt_pretty(f)?;
                }

                write!(f, "\n{:indent$}", "", indent = indent)?;
                write_syntax(f, "}", styles)
            }
            Json::Array(arr) => {
                if arr.is_empty() {
                    return write_syntax(f, "[]", styles);
                }

                // exclude if the field is in the exclude list
                let should_exclude = |index: usize| {
                    exclude.clone().any(|e| {
                        if let Some(KeyType::Index(i)) = e.get(0) {
                            *i == index && e.len() == 1
                        } else {
                            false
                        }
                    })
                };

                let mut non_nulls = arr
                    .iter()
                    .enumerate()
                    .filter(|(i, v)| !v.is_null() && !should_exclude(*i));
                let Some((index, value)) = non_nulls.next() else {
                    return write_syntax(f, "[]", styles);
                };

                write_syntax(f, "[", styles)?;
                write!(f, "\n{:indent$}", "", indent = (indent + 2))?;

                let mut value =
                    value
                        .indented(*indent + 2)
                        .exclude_fields(exclude.clone().filter_map(|e| {
                            if let Some(KeyType::Index(i)) = e.get(0) {
                                if *i == index && e.len() > 1 {
                                    return Some(&e[1..]);
                                }
                            }

                            None
                        }));
                value.styles = *styles;
                value.fmt_pretty(f)?;

                for (index, value) in non_nulls {
                    write_syntax(f, ",", styles)?;
                    write!(f, "\n{:indent$}", "", indent = (indent + 2))?;
                    let mut value =
                        value
                            .indented(*indent + 2)
                            .exclude_fields(exclude.clone().filter_map(|e| {
                                if let Some(KeyType::Index(i)) = e.get(0) {
                                    if *i == index && e.len() > 1 {
                                        return Some(&e[1..]);
                                    }
                                }

                                None
                            }));
                    value.styles = *styles;
                    value.fmt_pretty(f)?;
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
