use core::fmt;

use owo_colors::AnsiColors;
pub use owo_colors::OwoColorize as Colorize;

use super::*;
use crate::Json;

// used for displaying the formatted log to output
pub struct FormattedLog<'a> {
    pub(super) formatter: &'a Formatter,
    pub(super) json: &'a Json<'a>,
}

impl fmt::Display for FormattedLog<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.write_fmt(f) }
}

impl FormattedLog<'_> {
    pub fn write_fmt(&self, f: &mut impl fmt::Write) -> Result<(), fmt::Error> {
        let Self {
            formatter: Formatter { pieces, args },
            json,
        } = self;

        let mut used_fields = SmallVec::new();

        let mut piece_i = 0;
        while piece_i < pieces.len() {
            piece_i = write_piece(f, pieces, piece_i, args, json, false, &mut used_fields)?;
        }

        Ok(())
    }
}

fn write_piece<'a>(
    f: &mut impl fmt::Write,
    pieces: &'a Vec<Piece>,
    mut piece_i: usize,
    args: &'a Vec<Arg>,
    json: &'a Json<'a>,
    skip: bool,
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> Result<usize, fmt::Error> {
    use Piece::*;

    match &pieces[piece_i] {
        Literal(literal) => {
            if !skip {
                write!(f, "{}", literal)?
            }
        }
        Escaped(c) => {
            if !skip {
                write!(f, "{}", c)?
            }
        }
        Arg(i) => {
            if !skip {
                write_arg(f, &args[*i], json, used_fields)?
            }
        }
        CondStart(cond, i) => {
            let cond_matched = !skip && test_cond(*cond, args, *i, json, used_fields);
            let mut should_run = cond_matched;
            let mut else_cond_matched = false;

            piece_i += 1;
            while piece_i < pieces.len() {
                if let Piece::ElseCond(cond, i) = pieces[piece_i] {
                    if !skip && !cond_matched && !else_cond_matched {
                        should_run = test_cond(cond, args, i, json, used_fields);
                        else_cond_matched = true;
                    } else {
                        should_run = false;
                    }

                    piece_i += 1;
                } else if let Piece::Else = pieces[piece_i] {
                    if !skip && !should_run && !else_cond_matched {
                        should_run = true;
                        else_cond_matched = true;
                    } else {
                        should_run = false;
                    }

                    piece_i += 1;
                }

                if let Piece::CondEnd = pieces[piece_i] {
                    break;
                }

                piece_i = write_piece(f, pieces, piece_i, args, json, !should_run, used_fields)?;
            }
        }
        // Handled in the IfStart case above
        ElseCond(..) | Else | CondEnd => {}
    }

    Ok(piece_i + 1)
}

fn test_cond<'a>(
    cond: Cond,
    args: &[Arg],
    i: usize,
    json: &'a Json<'a>,
    used_fields: &SmallVec<[&'a Field; 5]>,
) -> bool {
    if let Cond::IfConfig(b) = cond {
        return b;
    }

    let (field_options, _) = &args[i];
    let mut val = &Json::Null;
    for field in field_options {
        val = json;

        match field {
            Field::Whole => return test_cond2(cond, json),
            Field::Rest => {
                // optimization
                // for `key` conditional, `rest` always exists
                // since it's the base object
                if cond == Cond::Key {
                    return true;
                } else {
                    let rest = get_rest(json, used_fields);
                    return test_cond2(cond, &rest);
                }
            }
            Field::Names(names) => {
                for arg in names {
                    match arg {
                        FieldType::Name(name) => {
                            val = val.get(name);
                        }
                        FieldType::Index(index) => {
                            val = val.get_i(*index);
                        }
                    }
                }
            }
        }

        if !val.is_null() {
            break;
        }
    }

    test_cond2(cond, val)
}

fn test_cond2(cond: Cond, json: &Json<'_>) -> bool {
    if json.is_null() {
        return false;
    }

    match cond {
        Cond::Key => true,
        Cond::If => {
            if json.is_array() || json.is_object() {
                !json.is_empty()
            } else if let Some(json) = json.as_str() {
                !json.is_empty()
            } else if let Some(json) = json.as_value() {
                !(json == "false"
                    || json == "0"
                    || json == "-0"
                    || json == "0n"
                    || json == "undefined"
                    || json == "NaN")
            } else {
                unreachable!("all cases checked")
            }
        }
        _ => unreachable!("checked above"),
    }
}

fn write_arg<'a>(
    f: &mut impl fmt::Write,
    (field_options, format): &'a (FieldOptions, Format),
    json: &'a Json<'a>,
    used_fields: &mut SmallVec<[&'a Field; 5]>,
) -> fmt::Result {
    let mut val = &Json::Null;

    for field in field_options {
        val = json;

        match field {
            Field::Whole => {
                return write_arg2(f, format, json);
            }
            Field::Rest => {
                let rest = get_rest(json, used_fields);
                return write_arg2(f, format, &rest);
            }
            Field::Names(names) => {
                for arg in names {
                    match arg {
                        FieldType::Name(name) => {
                            val = val.get(name);
                        }
                        FieldType::Index(index) => {
                            val = val.get_i(*index);
                        }
                    }
                }

                if !val.is_null() {
                    used_fields.push(field);
                    break;
                }
            }
        }
    }

    write_arg2(f, format, val)
}

fn write_arg2(f: &mut impl fmt::Write, format: &Format, json: &Json<'_>) -> fmt::Result {
    let Format {
        style,
        compact,
        is_json,
        is_level,
        indent,
        markup_styles: json_styles,
    } = format;
    let indent = *indent;
    let is_level = *is_level;

    if indent > 0 {
        write!(f, "{:indent$}", "", indent = indent)?;
    }

    if let Some(val) = json.as_str() {
        if let Some(style) = style {
            if is_level {
                match val {
                    "TRACE" | "trace" => write!(
                        f,
                        "{}",
                        val.style((*style).color(AnsiColors::Cyan).dimmed())
                    )?,
                    "DEBUG" | "debug" => {
                        write!(f, "{}", val.style((*style).color(AnsiColors::Green)))?
                    }
                    "INFO" | "info" => {
                        write!(f, " {}", val.style((*style).color(AnsiColors::Cyan)))?
                    }
                    "WARN" | "warn" => {
                        write!(f, " {}", val.style((*style).color(AnsiColors::Yellow)))?
                    }
                    "ERROR" | "error" => {
                        write!(f, "{}", val.style((*style).color(AnsiColors::Red)))?
                    }
                    _ => write!(f, "{}", val.style(*style))?,
                }
            } else {
                write!(f, "{}", val.style(*style))?;
            }
        } else {
            write!(f, "{}", val)?;
        }
    } else if let Some(val) = json.as_value() {
        if let Some(style) = style.as_ref() {
            write!(f, "{}", val.style(*style))?;
        } else {
            write!(f, "{}", val)?;
        }
    } else if json.is_object() || json.is_array() {
        // TODO: Implement formatting for objects
        match (is_json, compact) {
            (true, true) => {
                if style.is_some() {
                    write!(f, "{}", json.styled(*json_styles))?;
                } else {
                    write!(f, "{}", json)?;
                }
            }
            (true, false) => {
                if style.is_some() {
                    write!(f, "{:?}", json.indented(indent).styled(*json_styles))?;
                } else {
                    write!(f, "{:?}", json.indented(indent))?;
                }
            }
            (false, true) => {
                if style.is_some() {
                    write!(f, "{}", json.styled(*json_styles))?;
                } else {
                    write!(f, "{}", json)?;
                }
            }
            (false, false) => {
                if style.is_some() {
                    write!(f, "{:?}", json.indented(indent).styled(*json_styles))?;
                } else {
                    write!(f, "{:?}", json.indented(indent))?;
                }
            }
        }
    }

    Ok(())
}

pub fn get_rest<'a>(json: &'a Json<'a>, used_fields: &SmallVec<[&'a Field; 5]>) -> Json<'a> {
    let mut rest = json.clone();

    for field in used_fields.iter() {
        if let Field::Names(names) = field {
            let pointer = field_to_pointer(names);

            // TODO: SAFE?
            let ref_mut = unsafe { &mut *(&mut rest as *mut Json<'_>) };
            remove_field(ref_mut, &pointer);
        } else {
            // if whole or rest json were used,
            // there are no fields in rest json.
            rest = Json::Object(Default::default());
            break;
        }
    }

    rest
}

pub fn field_to_pointer(field: &FieldNames) -> String {
    let mut ret = String::new();

    for f in field.iter() {
        ret.push('/');
        match f {
            FieldType::Name(e) => ret.push_str(e),
            FieldType::Index(e) => ret.push_str(&e.to_string()),
        };
    }

    ret
}

fn remove_field<'a>(val: &'a mut Json<'a>, pointer: &str) {
    if pointer.is_empty() {
        val.replace(Json::Null);
        return;
    }

    if let Some(v) = val.pointer_mut(pointer) {
        v.replace(Json::Null);
    }
}
