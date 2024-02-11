use core::fmt;
use owo_colors::{
    colors::{Blue, BrightWhite, Green, White},
    OwoColorize, Style,
};
use std::iter::Peekable;

pub fn parse_json(input: &str) -> Result<Json<'_>, ParseError> {
    let mut json = Json::Null; // Start with a Null value
    json.parse_replace(input)?;
    Ok(json)
}

#[derive(Clone)]
pub enum Json<'a> {
    // first arg is the key value pairs, second is a list of keys used as cache for parse_replace
    Object(JsonObject<'a>),
    Array(Vec<Json<'a>>),
    String(&'a str),
    Value(&'a str),
    Null,
    NullPrevObject(JsonObject<'a>),
    NullPrevArray(Vec<Json<'a>>),
}

impl<'a> Json<'a> {
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Object(obj) => obj.get(key),
            _ => &Json::Null,
        }
    }

    pub fn get_mut(&'a mut self, key: &str) -> Option<&'a mut Json> {
        match self {
            Json::Object(obj) => obj.get_mut(key),
            _ => None,
        }
    }

    pub fn get_i(&self, index: usize) -> &Json {
        match self {
            Json::Array(arr) => arr.get(index).unwrap_or(&Json::Null),
            _ => &Json::Null,
        }
    }

    pub fn get_i_mut(&'a mut self, index: usize) -> Option<&'a mut Json> {
        match self {
            Json::Array(arr) => arr.get_mut(index),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(
            self,
            Json::Null | Json::NullPrevObject(_) | Json::NullPrevArray(_)
        )
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Json::Object(obj) => obj.is_empty(),
            Json::Array(arr) => arr.is_empty() || arr.iter().all(Json::is_null),
            Json::String(s) => s.is_empty(),
            Json::Value(_) => false,
            Json::Null | Json::NullPrevObject(_) | Json::NullPrevArray(_) => true,
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Json::Object(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Json::Array(_))
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Json::String(_))
    }

    pub fn is_value(&self) -> bool {
        matches!(self, Json::Value(_))
    }

    pub fn as_object(&self) -> Option<&JsonObject<'a>> {
        match self {
            Json::Object(obj) => Some(obj),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut JsonObject<'a>> {
        match self {
            Json::Object(obj) => Some(obj),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Json<'a>>> {
        match self {
            Json::Array(arr) => Some(arr),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Json<'a>>> {
        match self {
            Json::Array(arr) => Some(arr),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_value(&self) -> Option<&str> {
        match self {
            Json::Value(v) => Some(v),
            _ => None,
        }
    }

    // Replace self with a new value and return the previous value
    pub fn replace(&mut self, value: Json<'a>) -> Json<'a> {
        std::mem::replace(self, value)
    }

    pub fn parse_replace(&mut self, input: &'a str) -> Result<(), ParseError> {
        let mut chars = input.char_indices().peekable();
        self.parse_value_in_place(&mut chars, input)?;
        Ok(())
    }

    fn parse_value_in_place<I>(
        &mut self,
        chars: &mut Peekable<I>,
        input: &'a str,
    ) -> Result<(), ParseError>
    where
        I: Iterator<Item = (usize, char)>,
    {
        match chars.peek().map(|&(_, c)| c) {
            Some('{') => {
                if let Json::Object(obj) = self {
                    obj.parse_object_in_place(chars, input)?;
                } else {
                    let this = self.replace(Json::Null);
                    if let Json::NullPrevObject(mut obj) = this {
                        obj.parse_object_in_place(chars, input)?;
                        *self = Json::Object(obj);
                    } else {
                        let mut obj = JsonObject(Vec::new());
                        obj.parse_object_in_place(chars, input)?;
                        *self = Json::Object(obj);
                    }
                }
            }
            Some('[') => {
                if let Json::Array(arr) = self {
                    parse_array_in_place(arr, chars, input)?;
                } else {
                    let this = self.replace(Json::Null);
                    if let Json::NullPrevArray(mut arr) = this {
                        parse_array_in_place(&mut arr, chars, input)?;
                        *self = Json::Array(arr);
                    } else {
                        let mut arr = Vec::new();
                        parse_array_in_place(&mut arr, chars, input)?;
                        *self = Json::Array(arr);
                    }
                }
            }
            Some('"') => {
                *self = parse_string(chars, input)?;
            }
            Some('n') => {
                parse_null(chars, input)?;
                self.replace_with_null();
            }
            Some(']') => {
                return Err(ParseError {
                    message: "Unexpected closing bracket",
                    value: input.to_owned(),
                    index: chars
                        .peek()
                        .map(|&(i, _)| i)
                        .unwrap_or_else(|| input.len() - 1),
                })
            }
            Some('}') => {
                return Err(ParseError {
                    message: "Unexpected closing brace",
                    value: input.to_owned(),
                    index: chars
                        .peek()
                        .map(|&(i, _)| i)
                        .unwrap_or_else(|| input.len() - 1),
                })
            }
            Some(_) => {
                *self = parse_raw_value(chars, input)?;
            }
            None => {
                return Err(ParseError {
                    message: "Unexpected end of input",
                    value: input.to_owned(),
                    index: input.len(),
                })
            }
        }

        Ok(())
    }

    fn replace_with_null(&mut self) {
        let prev = self.replace(Json::Null);

        if let Json::Object(obj) = prev {
            *self = Json::NullPrevObject(obj);
        } else if let Json::Array(arr) = prev {
            *self = Json::NullPrevArray(arr);
        } else if matches!(prev, Json::NullPrevObject(_)) || matches!(prev, Json::NullPrevArray(_))
        {
            *self = prev;
        }
    }
}

#[derive(Clone)]
pub struct JsonObject<'a>(pub Vec<(&'a str, Json<'a>)>);

impl<'a> JsonObject<'a> {
    pub fn get(&self, key: &str) -> &Json {
        self.0
            .iter()
            .find(|(k, _)| k == &key)
            .map(|(_, v)| v)
            .unwrap_or(&Json::Null)
    }

    pub fn get_mut(&'a mut self, key: &str) -> Option<&'a mut Json> {
        self.0.iter_mut().find(|(k, _)| k == &key).map(|(_, v)| v)
    }

    pub fn insert(&mut self, key: &'a str, value: Json<'a>) {
        if let Some((_, v)) = self.0.iter_mut().find(|(k, _)| k == &key) {
            *v = value;
        } else {
            self.0.push((key, value));
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Json<'a>> {
        if let Some((_, val)) = self.0.iter_mut().find(|(k, _)| k == &key) {
            Some(val.replace(Json::Null))
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        if self.0.is_empty() {
            return true;
        }

        self.0.iter().all(|(_, v)| v.is_null())
    }

    pub fn iter(&self) -> std::slice::Iter<(&'a str, Json<'a>)> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<(&'a str, Json<'a>)> {
        self.0.iter_mut()
    }

    pub fn parse_insert(&mut self, key: &'a str, input: &'a str) -> Result<(), ParseError> {
        if let Some((old_key, value)) = self.0.iter_mut().find(|(k, _)| k == &key) {
            *old_key = key;
            value.parse_replace(input)?;
        } else {
            let mut new_value = Json::Null;
            new_value.parse_replace(input)?;

            self.0.push((key, new_value));
        }

        Ok(())
    }

    fn parse_object_in_place<I>(
        &mut self,
        chars: &mut Peekable<I>,
        input: &'a str,
    ) -> Result<(), ParseError>
    where
        I: Iterator<Item = (usize, char)>,
    {
        // Consume the opening '{'
        let Some((_, '{')) = chars.next() else {
            return Err(ParseError {
                message: "Object doesn't have a starting brace",
                value: input.to_owned(),
                index: 0,
            });
        };

        skip_whitespace(chars);
        if let Some((_, '}')) = chars.peek() {
            chars.next(); // Consume the closing '}'

            // Set values to Json::Null for keys not found in the input
            for (_, value) in self.iter_mut() {
                value.replace_with_null();
            }

            return Ok(());
        }

        let mut count = 0;

        loop {
            let Ok(Json::String(key)) = parse_string(chars, input) else {
                return Err(ParseError {
                    message: "Unexpected char in object",
                    value: input.to_owned(),
                    index: chars
                        .peek()
                        .map(|&(i, _)| i - 1)
                        .unwrap_or_else(|| input.len() - 1),
                });
            };

            skip_whitespace(chars);
            if chars.next().map(|(_, c)| c) != Some(':') {
                return Err(ParseError {
                    message: "Expected colon ':' after key in object",
                    value: input.to_owned(),
                    // Use the index right after the key, which should be the current position
                    index: chars
                        .peek()
                        .map(|&(i, _)| i - 1)
                        .unwrap_or_else(|| input.len() - 1),
                });
            }

            skip_whitespace(chars);
            if let Some((old_key, value)) = self.0.get_mut(count) {
                *old_key = key;
                value.parse_value_in_place(chars, input)?;
            } else {
                let mut new_value = Json::Null;
                new_value.parse_value_in_place(chars, input)?;
                self.0.push((key, new_value));
            }

            count += 1;

            skip_whitespace(chars);
            match chars.peek().map(|&(_, c)| c) {
                Some(',') => {
                    chars.next();
                    skip_whitespace(chars);
                } // Consume and continue
                Some('}') => {
                    chars.next(); // Consume the closing '}'

                    for (_, value) in self.iter_mut().skip(count) {
                        value.replace_with_null();
                    }

                    return Ok(());
                }
                _ => {
                    return Err(ParseError {
                        message: "Expected comma or closing brace '}' in object",
                        value: input.to_owned(),
                        index: chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len()),
                    })
                }
            }
        }
    }
}

fn parse_array_in_place<'a, I>(
    arr: &mut Vec<Json<'a>>,
    chars: &mut Peekable<I>,
    input: &'a str,
) -> Result<(), ParseError>
where
    I: Iterator<Item = (usize, char)>,
{
    chars.next(); // Consume the opening '['

    skip_whitespace(chars);
    if let Some((_, ']')) = chars.peek() {
        chars.next(); // Consume the closing ']'

        for value in arr.iter_mut() {
            value.replace_with_null();
        }

        return Ok(());
    }

    let mut count = 0;

    loop {
        if count < arr.len() {
            arr[count].parse_value_in_place(chars, input)?;
        } else {
            let mut new_element = Json::Null;
            new_element.parse_value_in_place(chars, input)?;
            arr.push(new_element);
        }
        count += 1;

        skip_whitespace(chars);
        match chars.peek().map(|&(_, c)| c) {
            Some(',') => {
                chars.next();
                skip_whitespace(chars);
            } // Consume and continue
            Some(']') => {
                chars.next(); // Consume the closing ']'

                for value in arr.iter_mut().skip(count) {
                    value.replace_with_null();
                }

                return Ok(());
            } // Handle in the next loop iteration
            _ => {
                return Err(ParseError {
                    message: "Expected comma or closing bracket ']' in array",
                    value: input.to_owned(),
                    // Use the current position as the error index
                    index: chars
                        .peek()
                        .map(|&(i, _)| i)
                        .unwrap_or_else(|| input.len() - 1),
                });
            }
        }
    }
}

fn parse_string<'a, I>(chars: &mut Peekable<I>, input: &'a str) -> Result<Json<'a>, ParseError>
where
    I: Iterator<Item = (usize, char)>,
{
    // Consume the opening quote
    let Some((start_index, '"')) = chars.next() else {
        return Err(ParseError {
            message: "Expected opening quote for string",
            value: input.to_owned(),
            index: input.len(),
        });
    };

    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok(Json::String(&input[start_index + 1..i])),
            '\\' => {
                chars.next(); // Skip the character following the escape
            }
            _ => {}
        }
    }

    Err(ParseError {
        message: "Closing quote not found for string started",
        value: input.to_owned(),
        index: start_index,
    })
}

fn parse_null<'a, I>(chars: &mut Peekable<I>, input: &'a str) -> Result<Json<'a>, ParseError>
where
    I: Iterator<Item = (usize, char)>,
{
    let start_index = chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len());
    if chars.next().map(|(_, c)| c) == Some('n')
        && chars.next().map(|(_, c)| c) == Some('u')
        && chars.next().map(|(_, c)| c) == Some('l')
        && chars.next().map(|(_, c)| c) == Some('l')
    {
        Ok(Json::Null)
    } else {
        Err(ParseError {
            message: "Invalid null value",
            value: input.to_owned(),
            // Point to the start of 'n' that led to expecting "null"
            index: start_index,
        })
    }
}

fn parse_raw_value<'a, I>(chars: &mut Peekable<I>, input: &'a str) -> Result<Json<'a>, ParseError>
where
    I: Iterator<Item = (usize, char)>,
{
    let start_index = chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len());
    while let Some(&(i, c)) = chars.peek() {
        if c == ',' || c == ']' || c == '}' {
            return Ok(Json::Value(&input[start_index..i]));
        }
        chars.next();
    }

    Ok(Json::Value(&input[start_index..]))
}

// skip whitespaces and return the number of characters skipped
fn skip_whitespace<I>(chars: &mut Peekable<I>)
where
    I: Iterator<Item = (usize, char)>,
{
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

impl Json<'_> {
    pub fn indented(&self, indent: usize) -> StyledJson {
        StyledJson {
            json: self,
            indent,
            styles: None,
        }
    }

    pub fn styled(&self, styles: MarkupStyles) -> StyledJson {
        StyledJson {
            json: self,
            indent: 0,
            styles: Some(styles),
        }
    }
}

pub struct StyledJson<'a> {
    json: &'a Json<'a>,
    indent: usize,
    styles: Option<MarkupStyles>,
}

impl StyledJson<'_> {
    pub fn indented(self, indent: usize) -> Self {
        Self { indent, ..self }
    }

    pub fn styled(self, styles: MarkupStyles) -> Self {
        Self {
            styles: Some(styles),
            ..self
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
    fn fmt_compact(
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

    fn fmt_pretty(
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

fn write_key(
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

fn write_value(
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

fn write_str(
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

fn write_syntax(
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

pub struct ParseError {
    pub message: &'static str,
    pub value: String,
    pub index: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Create a snippet from the input, showing up to 10 characters before and after the error index
        let start = self.index.saturating_sub(15);
        let end = (self.index + 10).min(self.value.len());
        let snippet = &self.value[start..end];

        write!(f, "{} at index {}: '{}'", self.message, self.index, snippet)
    }
}

impl std::fmt::Debug for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let snippet_length = 20;
        let start = self.index.saturating_sub(snippet_length);
        let end = (self.index + snippet_length).min(self.value.len());
        let snippet = &self.value[start..end];

        let caret_position = self.index.saturating_sub(start) + 1;

        write!(
            f,
            "{} at index {}:\n'{}'\n{:>width$}",
            self.message,
            self.index,
            snippet,
            "^",                        // Caret pointing to the error location
            width = caret_position + 1, // Correct alignment for the caret
        )
    }
}
impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let test_cases = vec![
            r#"{"key": "value"}"#,
            r#"{"escaped": "This is a \"test\""}"#,
            r#"{"nested": {"array": [1, "two", null], "emptyObj": {}, "bool": true}}"#,
            r#"["mixed", 123, {"obj": "inside array"}]"#,
            r#"{}"#,
            r#"[]"#,
        ];

        for case in test_cases {
            match parse_json(case) {
                Ok(parsed) => println!("Parsed JSON: {:#?}", parsed),
                Err(e) => println!("Failed to parse JSON: {}", e),
            }
        }

        let arr = parse_json(r#"["mixed", 123, {"obj": "inside array"}]"#).unwrap();
        println!("Array: {:#?}", arr);
        assert_eq!(arr.get_i(2).get("obj").as_value(), Some("\"inside array\""));
    }

    #[test]
    fn invalid() {
        let test_cases = vec![
            (
                r#"{"key": "value"         "#,
                "Missing Closing Brace for an Object",
            ),
            (
                r#"{"key": "value         }"#,
                "Missing Closing Quote for a String",
            ),
            (r#"{"key"     ,     "value"}"#, "Missing Colon in an Object"),
            (
                r#"{"key1": "value1", "key2": "value2"       ,          }"#,
                "Extra Comma in an Object",
            ),
            (r#"{key: "value"}"#, "Unquoted Key"),
            (
                r#"{"array": [1, 2, "missing bracket"        ,    }        "#,
                "Unclosed Array",
            ),
        ];

        for (json_str, description) in test_cases {
            println!("Testing case: {}", description);
            match parse_json(json_str) {
                Ok(_) => println!("No error detected, but expected an error."),
                Err(e) => {
                    println!("Error (Display): {}", e);
                    println!("Error (Debug):\n{:?}", e);
                }
            }
            println!("---------------------------------------\n");
        }
    }
}
