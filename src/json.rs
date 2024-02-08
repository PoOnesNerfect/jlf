use bumpalo::Bump;
use core::fmt;
use smallvec::SmallVec;
use std::iter::Peekable;

pub fn parse_json<'a>(input: &'a str, bump: &'a Bump) -> Result<Json<'a>, ParseError> {
    let mut json = Json::new(bump);
    json.parse_replace(input)?;
    Ok(json)
}

pub struct Json<'a> {
    inner: JsonInner<'a>,
    bump: &'a bumpalo::Bump,
}

pub enum JsonInner<'a> {
    // first arg is the key value pairs, second is a list of keys used as cache for parse_replace
    Object(SmallVec<[(&'a str, &'a mut JsonInner<'a>); 20]>),
    Array(SmallVec<[&'a mut JsonInner<'a>; 20]>),
    String(&'a str),
    Value(&'a str),
    Null,
    NullPrevObject(SmallVec<[(&'a str, &'a mut JsonInner<'a>); 20]>),
    NullPrevArray(SmallVec<[&'a mut JsonInner<'a>; 20]>),
}

impl<'a> Json<'a> {
    pub fn new(bump: &'a Bump) -> Self {
        Json {
            inner: JsonInner::Null,
            bump,
        }
    }
}

impl<'a> Json<'a> {
    // pub fn get(&self, key: &str) -> &JsonInner<'a> {
    //     match self.inner {
    //         JsonInner::Object(obj) => obj
    //             .iter()
    //             .find(|(k, _)| k == &key)
    //             .map(|(_, &v)| &*v)
    //             .unwrap_or(&JsonInner::Null),
    //         _ => &JsonInner::Null,
    //     }
    // }

    // pub fn get_i(&self, index: usize) -> &Json {
    //     match self {
    //         JsonInner::Array(arr) => arr.get(index).unwrap_or(&JsonInner::Null),
    //         _ => &JsonInner::Null,
    //     }
    // }

    // pub fn is_null(&self) -> bool {
    //     matches!(
    //         self,
    //         JsonInner::Null | JsonInner::NullPrevObject(_) | JsonInner::NullPrevArray(_)
    //     )
    // }

    // pub fn is_empty(&self) -> bool {
    //     match self {
    //         JsonInner::Object(obj) => obj.is_empty(),
    //         JsonInner::Array(arr) => arr.is_empty(),
    //         JsonInner::String(s) => s.is_empty(),
    //         JsonInner::Value(_) => false,
    //         JsonInner::Null | JsonInner::NullPrevObject(_) | JsonInner::NullPrevArray(_) => true,
    //     }
    // }

    // pub fn is_object(&self) -> bool {
    //     matches!(self, JsonInner::Object(_))
    // }

    // pub fn is_array(&self) -> bool {
    //     matches!(self, JsonInner::Array(_))
    // }

    // pub fn is_value(&self) -> bool {
    //     matches!(self, JsonInner::Value(_))
    // }

    pub fn as_object(&self) -> Option<impl Iterator<Item = &(&'a str, &'a mut JsonInner<'a>)>> {
        match &self.inner {
            JsonInner::Object(obj) => Some(obj.iter()),
            _ => None,
        }
    }

    // pub fn as_array(&self) -> Option<&Vec<Json<'a>>> {
    //     match self {
    //         JsonInner::Array(arr) => Some(arr),
    //         _ => None,
    //     }
    // }

    // pub fn as_object_mut(&mut self) -> Option<&mut Vec<(&'a str, Json<'a>)>> {
    //     match self {
    //         JsonInner::Object(obj) => Some(obj),
    //         _ => None,
    //     }
    // }

    // pub fn as_array_mut(&mut self) -> Option<&mut Vec<Json<'a>>> {
    //     match self {
    //         JsonInner::Array(arr) => Some(arr),
    //         _ => None,
    //     }
    // }

    pub fn as_str(&self) -> Option<&str> {
        match &self.inner {
            JsonInner::String(s) => Some(s),
            _ => None,
        }
    }

    // pub fn as_value(&self) -> Option<&str> {
    //     match self {
    //         JsonInner::Value(v) => Some(v),
    //         _ => None,
    //     }
    // }

    pub fn parse_replace(&mut self, input: &'a str) -> Result<(), ParseError> {
        let Self { inner, bump } = self;

        let mut chars = input.char_indices().peekable();
        inner.parse_value_in_place(&mut chars, input, bump)?;
        Ok(())
    }
}

impl<'a> JsonInner<'a> {
    pub fn as_str(&self) -> Option<&str> {
        match &self {
            JsonInner::String(s) => Some(s),
            _ => None,
        }
    }

    fn parse_value_in_place<I>(
        &mut self,
        chars: &mut Peekable<I>,
        input: &'a str,
        bump: &'a Bump,
    ) -> Result<(), ParseError>
    where
        I: Iterator<Item = (usize, char)>,
    {
        match chars.peek().map(|&(_, c)| c) {
            Some('{') => {
                if let JsonInner::Object(obj) = self {
                    parse_object_in_place(obj, chars, input, bump)?;
                } else {
                    let this = self.replace(JsonInner::Null);
                    if let JsonInner::NullPrevObject(mut obj) = this {
                        parse_object_in_place(&mut obj, chars, input, bump)?;
                        *self = JsonInner::Object(obj);
                    } else {
                        let mut obj = SmallVec::new();
                        parse_object_in_place(&mut obj, chars, input, bump)?;
                        *self = JsonInner::Object(obj);
                    }
                }
            }
            Some('[') => {
                if let JsonInner::Array(arr) = self {
                    parse_array_in_place(arr, chars, input, bump)?;
                } else {
                    let this = self.replace(JsonInner::Null);
                    if let JsonInner::NullPrevArray(mut arr) = this {
                        parse_array_in_place(&mut arr, chars, input, bump)?;
                        *self = JsonInner::Array(arr);
                    } else {
                        let mut arr = SmallVec::new();
                        parse_array_in_place(&mut arr, chars, input, bump)?;
                        *self = JsonInner::Array(arr);
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
        let prev = self.replace(JsonInner::Null);

        if let JsonInner::Object(obj) = prev {
            *self = JsonInner::NullPrevObject(obj);
        } else if let JsonInner::Array(arr) = prev {
            *self = JsonInner::NullPrevArray(arr);
        } else if matches!(prev, JsonInner::NullPrevObject(_))
            || matches!(prev, JsonInner::NullPrevArray(_))
        {
            *self = prev;
        }
    }

    // Replace self with a new value and return the previous value
    pub fn replace(&mut self, value: JsonInner<'a>) -> JsonInner<'a> {
        std::mem::replace(self, value)
    }
}

fn parse_object_in_place<'a, I>(
    pairs: &mut SmallVec<[(&'a str, &mut JsonInner<'a>); 20]>,
    chars: &mut Peekable<I>,
    input: &'a str,
    bump: &'a Bump,
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

        // Set values to JsonInner::Null for keys not found in the input
        for (_, value) in pairs.iter_mut() {
            value.replace_with_null();
        }

        return Ok(());
    }

    let mut count = 0;

    loop {
        let Ok(JsonInner::String(key)) = parse_string(chars, input) else {
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
        if let Some((old_key, value)) = pairs.get_mut(count) {
            *old_key = key;
            value.parse_value_in_place(chars, input, bump)?;
        } else {
            let mut new_value = bump.alloc(JsonInner::Null);
            new_value.parse_value_in_place(chars, input, bump)?;
            pairs.push((key, new_value));
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

                for (_, value) in pairs.iter_mut().skip(count) {
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

fn parse_array_in_place<'a, I>(
    arr: &mut SmallVec<[&'a mut JsonInner<'a>; 20]>,
    chars: &mut Peekable<I>,
    input: &'a str,
    bump: &'a Bump,
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
            arr[count].parse_value_in_place(chars, input, bump)?;
        } else {
            let mut new_element = bump.alloc(JsonInner::Null);
            new_element.parse_value_in_place(chars, input, bump)?;
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

fn parse_string<'a, I>(chars: &mut Peekable<I>, input: &'a str) -> Result<JsonInner<'a>, ParseError>
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
            '"' => return Ok(JsonInner::String(&input[start_index + 1..i])),
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

fn parse_null<'a, I>(chars: &mut Peekable<I>, input: &'a str) -> Result<JsonInner<'a>, ParseError>
where
    I: Iterator<Item = (usize, char)>,
{
    let start_index = chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len());
    if chars.next().map(|(_, c)| c) == Some('n')
        && chars.next().map(|(_, c)| c) == Some('u')
        && chars.next().map(|(_, c)| c) == Some('l')
        && chars.next().map(|(_, c)| c) == Some('l')
    {
        Ok(JsonInner::Null)
    } else {
        Err(ParseError {
            message: "Invalid null value",
            value: input.to_owned(),
            // Point to the start of 'n' that led to expecting "null"
            index: start_index,
        })
    }
}

fn parse_raw_value<'a, I>(
    chars: &mut Peekable<I>,
    input: &'a str,
) -> Result<JsonInner<'a>, ParseError>
where
    I: Iterator<Item = (usize, char)>,
{
    let start_index = chars.peek().map(|&(i, _)| i).unwrap_or_else(|| input.len());
    while let Some(&(i, c)) = chars.peek() {
        if c == ',' || c == ']' || c == '}' {
            return Ok(JsonInner::Value(&input[start_index..i]));
        }
        chars.next();
    }

    Ok(JsonInner::Value(&input[start_index..]))
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

// impl<'a> fmt::Display for Json<'a> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
//         match self.inner {
//             JsonInner::Object(obj) => {
//                 write!(f, "{{")?;

//                 for (key, value) in obj.iter().take(obj.len().saturating_sub(1)) {
//                     if !value.is_null() {
//                         write!(f, "\"{}\": ", key)?;
//                         fmt::Display::fmt(&value, f)?;
//                         write!(f, ", ")?;
//                     }
//                 }

//                 if let Some((key, value)) = obj.last() {
//                     if !value.is_null() {
//                         write!(f, "\"{}\": ", key)?;
//                         fmt::Display::fmt(&value, f)?;
//                     }
//                 }

//                 write!(f, "}}")
//             }
//             JsonInner::Array(arr) => {
//                 write!(f, "[")?;
//                 for value in arr.iter().take(arr.len().saturating_sub(1)) {
//                     if !value.is_null() {
//                         fmt::Display::fmt(&value, f)?;
//                         write!(f, ", ")?;
//                     }
//                 }
//                 if let Some(value) = arr.last() {
//                     if !value.is_null() {
//                         fmt::Display::fmt(&value, f)?;
//                     }
//                 }

//                 write!(f, "]")
//             }
//             JsonInner::String(v) => write!(f, "\"{}\"", v),
//             JsonInner::Value(v) => write!(f, "{}", v),
//             JsonInner::Null | JsonInner::NullPrevObject(_) | JsonInner::NullPrevArray(_) => {
//                 write!(f, "null")
//             }
//         }
//     }
// }

// impl<'a> fmt::Debug for Json<'a> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
//         self.fmt_pretty(f, 0)
//     }
// }

// impl Json<'_> {
//     fn fmt_pretty(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> Result<(), fmt::Error> {
//         match self {
//             JsonInner::Object(obj) => {
//                 if obj.is_empty() {
//                     return write!(f, "{{}}");
//                 }

//                 let mut non_nulls = obj.iter().filter(|(_, v)| !v.is_null());
//                 let Some((key, value)) = non_nulls.next() else {
//                     return write!(f, "{{}}");
//                 };
//                 write!(f, "{{\n")?;
//                 write!(f, "{:indent$}\"{}\": ", "", key, indent = (indent + 2))?;
//                 value.fmt_pretty(f, indent + 2)?;

//                 for (key, value) in non_nulls {
//                     write!(f, ",\n")?;
//                     write!(f, "{:indent$}\"{}\": ", "", key, indent = (indent + 2))?;
//                     value.fmt_pretty(f, indent + 2)?;
//                 }

//                 write!(f, "\n{:indent$}}}", "", indent = indent)
//             }
//             JsonInner::Array(arr) => {
//                 if arr.is_empty() {
//                     return write!(f, "[]");
//                 }

//                 let mut non_nulls = arr.iter().filter(|v| !v.is_null());
//                 let Some(value) = non_nulls.next() else {
//                     return write!(f, "[]");
//                 };
//                 write!(f, "[\n")?;
//                 write!(f, "{:indent$}", "", indent = (indent + 2))?;
//                 value.fmt_pretty(f, indent + 2)?;

//                 for value in non_nulls {
//                     write!(f, ",\n")?;
//                     write!(f, "{:indent$}", "", indent = (indent + 2))?;
//                     value.fmt_pretty(f, indent + 2)?;
//                 }
//                 write!(f, "\n{:indent$}]", "", indent = indent)
//             }
//             JsonInner::String(v) => write!(f, "\"{}\"", v),
//             JsonInner::Value(v) => write!(f, "{}", v),
//             JsonInner::Null | JsonInner::NullPrevObject(_) | JsonInner::NullPrevArray(_) => {
//                 write!(f, "null")
//             }
//         }
//     }
// }

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

    // #[test]
    // fn basic() {
    //     let test_cases = vec![
    //         r#"{"key": "value"}"#,
    //         r#"{"escaped": "This is a \"test\""}"#,
    //         r#"{"nested": {"array": [1, "two", null], "emptyObj": {}, "bool": true}}"#,
    //         r#"["mixed", 123, {"obj": "inside array"}]"#,
    //         r#"{}"#,
    //         r#"[]"#,
    //     ];

    //     for case in test_cases {
    //         match parse_json(case) {
    //             Ok(parsed) => println!("Parsed JSON: {:#?}", parsed),
    //             Err(e) => println!("Failed to parse JSON: {}", e),
    //         }
    //     }

    //     let arr = parse_json(r#"["mixed", 123, {"obj": "inside array"}]"#).unwrap();
    //     println!("Array: {:#?}", arr);
    //     assert_eq!(arr.get_i(2).get("obj").as_value(), Some("\"inside array\""));
    // }

    // #[test]
    // fn invalid() {
    //     let test_cases = vec![
    //         (
    //             r#"{"key": "value"         "#,
    //             "Missing Closing Brace for an Object",
    //         ),
    //         (
    //             r#"{"key": "value         }"#,
    //             "Missing Closing Quote for a String",
    //         ),
    //         (r#"{"key"     ,     "value"}"#, "Missing Colon in an Object"),
    //         (
    //             r#"{"key1": "value1", "key2": "value2"       ,          }"#,
    //             "Extra Comma in an Object",
    //         ),
    //         (r#"{key: "value"}"#, "Unquoted Key"),
    //         (
    //             r#"{"array": [1, 2, "missing bracket"        ,    }        "#,
    //             "Unclosed Array",
    //         ),
    //     ];

    //     for (json_str, description) in test_cases {
    //         println!("Testing case: {}", description);
    //         match parse_json(json_str) {
    //             Ok(_) => println!("No error detected, but expected an error."),
    //             Err(e) => {
    //                 println!("Error (Display): {}", e);
    //                 println!("Error (Debug):\n{:?}", e);
    //             }
    //         }
    //         println!("---------------------------------------\n");
    //     }
    // }
}
