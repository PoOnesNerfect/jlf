use std::iter::Peekable;

pub fn parse_json<'a>(input: &'a str) -> Result<Json<'a>, ParseError<'a>> {
    let mut indicies = JsonIndices::Null; // Start with a Null value
    indicies.parse_replace(input)?;
    Ok(Json {
        raw: input,
        indicies,
    })
}

#[derive(Debug, Clone)]
pub struct Json<'a> {
    pub raw: &'a str,
    pub indicies: JsonIndices,
}

impl<'a> Json<'a> {
    pub fn as_ref(&self) -> JsonRef {
        JsonRef {
            raw: self.raw,
            indicies: &self.indicies,
        }
    }

    pub fn get(&'a self, key: &str) -> JsonRef<'a> {
        self.as_ref().get(key)
    }

    pub fn get_i(&'a self, index: usize) -> JsonRef<'a> {
        self.as_ref().get_i(index)
    }

    pub fn is_null(&self) -> bool {
        matches!(self.indicies, JsonIndices::Null)
    }

    pub fn is_empty(&self) -> bool {
        match &self.indicies {
            JsonIndices::Object(obj, _) => obj.is_empty(),
            JsonIndices::Array(arr) => arr.is_empty(),
            JsonIndices::Null => true,
            _ => false,
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self.indicies, JsonIndices::Object(_, _))
    }

    pub fn is_array(&self) -> bool {
        matches!(self.indicies, JsonIndices::Array(_))
    }

    pub fn is_value(&self) -> bool {
        matches!(self.indicies, JsonIndices::Value(_))
    }

    pub fn as_value(&self) -> Option<&str> {
        match self.indicies {
            JsonIndices::Value((s, e)) => Some(&self.raw[s..e]),
            _ => None,
        }
    }

    #[inline]
    pub fn parse_replace(&mut self, input: &'a str) -> Result<(), ParseError<'a>> {
        self.indicies.parse_replace(input)?;
        self.raw = input;
        Ok(())
    }

    // Replace self with a new value and return the previous value
    pub fn replace(&mut self, value: Self) -> Self {
        std::mem::replace(self, value)
    }

    // Replace self with JsonIndices::Null and return the previous value
    pub fn take(&mut self) -> Self {
        self.replace(Self {
            raw: self.raw,
            indicies: JsonIndices::Null,
        })
    }
}

#[derive(Debug, Clone)]
pub struct JsonRef<'a> {
    pub raw: &'a str,
    pub indicies: &'a JsonIndices,
}

#[derive(Debug, Clone, Default)]
pub enum JsonIndices {
    // first arg is the key value pairs, second is a list of keys used as cache for parse_in_place
    Object(Vec<((usize, usize), JsonIndices)>, Vec<(usize, usize)>),
    Array(Vec<JsonIndices>),
    Value((usize, usize)),
    #[default]
    Null,
}

impl<'a> JsonRef<'a> {
    pub fn get(&self, key: &str) -> Self {
        let indices = match self.indicies {
            JsonIndices::Object(obj, _) => obj
                .iter()
                .find(|((s, e), _)| &self.raw[*s..*e] == key)
                .map(|(_, v)| v)
                .unwrap_or(&JsonIndices::Null),
            _ => &JsonIndices::Null,
        };

        Self {
            raw: self.raw,
            indicies: indices,
        }
    }

    pub fn get_i(&self, index: usize) -> Self {
        let indices = match self.indicies {
            JsonIndices::Array(arr) => arr.get(index).unwrap_or(&JsonIndices::Null),
            _ => &JsonIndices::Null,
        };

        Self {
            raw: self.raw,
            indicies: indices,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self.indicies, JsonIndices::Null)
    }

    pub fn is_empty(&self) -> bool {
        match self.indicies {
            JsonIndices::Object(obj, _) => obj.is_empty(),
            JsonIndices::Array(arr) => arr.is_empty(),
            JsonIndices::Null => true,
            _ => false,
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self.indicies, JsonIndices::Object(_, _))
    }

    pub fn is_array(&self) -> bool {
        matches!(self.indicies, JsonIndices::Array(_))
    }

    pub fn is_value(&self) -> bool {
        matches!(self.indicies, JsonIndices::Value(_))
    }

    pub fn as_value(&self) -> Option<&str> {
        match self.indicies {
            JsonIndices::Value((s, e)) => Some(&self.raw[*s..*e]),
            _ => None,
        }
    }
}

impl JsonIndices {
    #[inline]
    fn parse_replace<'a>(&mut self, input: &'a str) -> Result<(), ParseError<'a>> {
        let mut chars = input.char_indices().peekable();
        self.parse_value_in_place(&mut chars, input)?;
        Ok(())
    }

    fn parse_value_in_place<'a, I>(
        &mut self,
        chars: &mut Peekable<I>,
        input: &'a str,
    ) -> Result<(), ParseError<'a>>
    where
        I: Iterator<Item = (usize, char)>,
    {
        match chars.peek().map(|&(_, c)| c) {
            Some('{') => {
                if let JsonIndices::Object(obj, keys) = self {
                    parse_object_in_place(obj, chars, input, keys)?;
                } else {
                    let mut obj = Vec::new();
                    let mut keys = Vec::new();
                    parse_object_in_place(&mut obj, chars, input, &mut keys)?;
                    *self = JsonIndices::Object(obj, keys);
                }
            }
            Some('[') => {
                if let JsonIndices::Array(arr) = self {
                    parse_array_in_place(arr, chars, input)?;
                } else {
                    let mut arr = Vec::new();
                    parse_array_in_place(&mut arr, chars, input)?;
                    *self = JsonIndices::Array(arr);
                }
            }
            Some('"') => {
                *self = parse_string(chars, input)?;
            }
            Some('n') => {
                *self = parse_null(chars, input)?;
            }
            Some(']') => {
                return Err(ParseError {
                    message: "Unexpected closing bracket",
                    value: input,
                    index: chars
                        .peek()
                        .map(|&(i, _)| i)
                        .unwrap_or_else(|| input.len() - 1),
                })
            }
            Some('}') => {
                return Err(ParseError {
                    message: "Unexpected closing brace",
                    value: input,
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
                    value: input,
                    index: input.len(),
                })
            }
        }

        Ok(())
    }
}

fn parse_object_in_place<'a, I>(
    pairs: &mut Vec<((usize, usize), JsonIndices)>,
    chars: &mut Peekable<I>,
    input: &'a str,
    new_keys: &mut Vec<(usize, usize)>,
) -> Result<(), ParseError<'a>>
where
    I: Iterator<Item = (usize, char)>,
{
    // Consume the opening '{'
    let Some((_, '{')) = chars.next() else {
        return Err(ParseError {
            message: "Object doesn't have a starting brace",
            value: input,
            index: 0,
        });
    };

    skip_whitespace(chars);
    if let Some((_, '}')) = chars.peek() {
        chars.next(); // Consume the closing '}'

        // Set values to JsonIndices::Null for keys not found in the input
        for (_, value) in pairs.iter_mut() {
            *value = JsonIndices::Null;
        }

        return Ok(());
    }

    new_keys.clear();

    loop {
        let Ok(JsonIndices::Value(key_in_quotes)) = parse_string(chars, input) else {
            return Err(ParseError {
                message: "Unexpected char in object",
                value: input,
                index: chars
                    .peek()
                    .map(|&(i, _)| i - 1)
                    .unwrap_or_else(|| input.len() - 1),
            });
        };

        let key = (key_in_quotes.0 + 1, key_in_quotes.1 - 1);

        skip_whitespace(chars);
        if chars.next().map(|(_, c)| c) != Some(':') {
            return Err(ParseError {
                message: "Expected colon ':' after key in object",
                value: input,
                // Use the index right after the key, which should be the current position
                index: chars
                    .peek()
                    .map(|&(i, _)| i - 1)
                    .unwrap_or_else(|| input.len() - 1),
            });
        }

        skip_whitespace(chars);
        if let Some(value) = pairs.iter_mut().find(|(k, _)| k == &key).map(|(_, v)| v) {
            value.parse_value_in_place(chars, input)?;
            new_keys.push(key);
        } else {
            let mut new_value = JsonIndices::Null;
            new_value.parse_value_in_place(chars, input)?;
            pairs.push((key, new_value));
            new_keys.push(key);
        }

        skip_whitespace(chars);
        match chars.peek().map(|&(_, c)| c) {
            Some(',') => {
                chars.next();
                skip_whitespace(chars);
            } // Consume and continue
            Some('}') => {
                chars.next(); // Consume the closing '}'

                for (key, value) in pairs {
                    if !new_keys.contains(key) {
                        *value = JsonIndices::Null;
                    }
                }

                return Ok(());
            }
            _ => {
                return Err(ParseError {
                    message: "Expected comma or closing brace '}' in object",
                    value: input,
                    index: chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len()),
                })
            }
        }
    }
}

fn parse_array_in_place<'a, I>(
    arr: &mut Vec<JsonIndices>,
    chars: &mut Peekable<I>,
    input: &'a str,
) -> Result<(), ParseError<'a>>
where
    I: Iterator<Item = (usize, char)>,
{
    chars.next(); // Consume the opening '['

    skip_whitespace(chars);
    if let Some((_, ']')) = chars.peek() {
        chars.next(); // Consume the closing ']'

        for value in arr.iter_mut() {
            *value = JsonIndices::Null;
        }

        return Ok(());
    }

    let mut count = 0;

    loop {
        if count < arr.len() {
            arr[count].parse_value_in_place(chars, input)?;
        } else {
            let mut new_element = JsonIndices::Null;
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
                    *value = JsonIndices::Null;
                }

                return Ok(());
            } // Handle in the next loop iteration
            _ => {
                return Err(ParseError {
                    message: "Expected comma or closing bracket ']' in array",
                    value: input,
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

fn parse_string<'a, I>(
    chars: &mut Peekable<I>,
    input: &'a str,
) -> Result<JsonIndices, ParseError<'a>>
where
    I: Iterator<Item = (usize, char)>,
{
    let start_index = chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len());

    // Consume the opening quote
    let Some((_, '"')) = chars.next() else {
        return Err(ParseError {
            message: "Expected opening quote for string",
            value: input,
            index: start_index,
        });
    };

    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Ok(JsonIndices::Value((start_index, i + 1))),
            '\\' => {
                chars.next(); // Skip the character following the escape
            }
            _ => {}
        }
    }

    Err(ParseError {
        message: "Closing quote not found for string started",
        value: input,
        index: start_index,
    })
}

fn parse_null<'a, I>(chars: &mut Peekable<I>, input: &'a str) -> Result<JsonIndices, ParseError<'a>>
where
    I: Iterator<Item = (usize, char)>,
{
    let start_index = chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len());
    if chars.next().map(|(_, c)| c) == Some('n')
        && chars.next().map(|(_, c)| c) == Some('u')
        && chars.next().map(|(_, c)| c) == Some('l')
        && chars.next().map(|(_, c)| c) == Some('l')
    {
        Ok(JsonIndices::Null)
    } else {
        Err(ParseError {
            message: "Invalid null value",
            value: input,
            // Point to the start of 'n' that led to expecting "null"
            index: start_index,
        })
    }
}

fn parse_raw_value<'a, I>(
    chars: &mut Peekable<I>,
    input: &'a str,
) -> Result<JsonIndices, ParseError<'a>>
where
    I: Iterator<Item = (usize, char)>,
{
    let start_index = chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len());
    while let Some(&(i, c)) = chars.peek() {
        if c == ',' || c == ']' || c == '}' {
            return Ok(JsonIndices::Value((start_index, i)));
        }
        chars.next();
    }

    Ok(JsonIndices::Value((start_index, input.len())))
}

// skip whitespaces and return the number of characters skipped
fn skip_whitespace<'a, I>(chars: &mut Peekable<I>)
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

pub struct ParseError<'a> {
    pub message: &'static str,
    pub value: &'a str,
    pub index: usize,
}

impl std::fmt::Display for ParseError<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Create a snippet from the input, showing up to 10 characters before and after the error index
        let start = self.index.saturating_sub(15);
        let end = (self.index + 10).min(self.value.len());
        let snippet = &self.value[start..end];

        write!(f, "{} at index {}: '{}'", self.message, self.index, snippet)
    }
}

impl std::fmt::Debug for ParseError<'_> {
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
impl std::error::Error for ParseError<'_> {}

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
