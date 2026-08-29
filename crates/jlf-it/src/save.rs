//! Persisting a built command as a `[recipe.NAME]` block.

use std::{fs, path::PathBuf};

use etcetera::{choose_base_strategy, BaseStrategy};

/// Candidate config files to save into, most local first.
pub struct SaveTarget {
    pub label: String,
    pub path: PathBuf,
}

/// The places a recipe can be saved: the workspace `jlf.toml`/`.jlf.toml` (if
/// one exists, else a new `./.jlf.toml`) and the user config `jlf/config.toml`.
pub fn targets() -> Vec<SaveTarget> {
    let mut out = Vec::new();

    let ws = ["jlf.toml", ".jlf.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(".jlf.toml"));
    out.push(SaveTarget {
        label: format!("this workspace ({})", ws.display()),
        path: ws,
    });

    if let Ok(strategy) = choose_base_strategy() {
        let mut p = strategy.config_dir();
        p.push("jlf");
        p.push("config.toml");
        out.push(SaveTarget {
            label: format!("your config ({})", p.display()),
            path: p,
        });
    }
    out
}

/// Append a recipe block to `path` (creating the file and parent dirs). Returns
/// whether an existing `[recipe.NAME]` was detected (so the caller can warn).
pub fn append_recipe(
    path: &PathBuf,
    name: &str,
    block: &str,
) -> std::io::Result<bool> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            fs::create_dir_all(dir)?;
        }
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    let duplicate = existing.contains(&format!("[recipe.{name}]"));

    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(block);
    fs::write(path, out)?;
    Ok(duplicate)
}
