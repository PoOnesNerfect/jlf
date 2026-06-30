use jlf_core::Json;

/// Resolve a dotted/indexed field path (e.g. `data.user.id`, `items.0`) against
/// a parsed record, mirroring the CLI's resolution rules.
pub fn resolve<'a>(json: &'a Json<'a>, path: &[String]) -> &'a Json<'a> {
    let mut cur = json;
    for seg in path {
        cur = match seg.parse::<usize>() {
            Ok(i) => cur.get_i(i),
            Err(_) => cur.get(seg),
        };
    }
    cur
}

/// A JSON scalar (string, number, bool) as a borrowed `&str`, if it is one.
pub fn scalar<'a>(json: &'a Json<'a>) -> Option<&'a str> {
    json.as_str().or_else(|| json.as_value())
}

/// Split a dotted field path into segments.
pub fn path(field: &str) -> Vec<String> {
    field.split('.').map(str::to_owned).collect()
}
