//! Persisting the current view as a `[recipe.NAME]` block in the workspace
//! config, so a filter/redaction assembled interactively can be reused from the
//! CLI as `jlf @name`.

use std::{fs, path::PathBuf};

/// Append a recipe built from the current `filter` text and `redact` globs to
/// the nearest workspace config (`jlf.toml`/`.jlf.toml`, created if absent).
/// Returns the file written and whether a recipe of that name already existed.
pub fn save_recipe(
    name: &str,
    filter: &str,
    redact: &[String],
) -> std::io::Result<(PathBuf, bool)> {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut block = format!("[recipe.{name}]\n");
    if !filter.trim().is_empty() {
        block.push_str(&format!("filter = \"{}\"\n", esc(filter.trim())));
    }
    if !redact.is_empty() {
        block.push_str(&format!("redact = \"{}\"\n", esc(&redact.join(","))));
    }

    let path = ["jlf.toml", ".jlf.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(".jlf.toml"));

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let duplicate = existing.contains(&format!("[recipe.{name}]"));

    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    fs::write(&path, out)?;
    Ok((path, duplicate))
}
