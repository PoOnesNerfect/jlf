use std::{fs, path::PathBuf};

use etcetera::{choose_base_strategy, BaseStrategy};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct ConfigFile {
    pub config: Config,
    pub variables: Vec<(String, String)>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub compact: Option<bool>,
    pub no_color: Option<bool>,
    pub strict: Option<bool>,
}

pub fn get_config() -> color_eyre::Result<ConfigFile> {
    let config_file = config_dir().join("config.toml");
    let mut config = if config_file.exists() {
        let config_raw = fs::read_to_string(config_file)?;
        toml::from_str(&config_raw)?
    } else {
        ConfigFile::default()
    };

    let ws_dir = find_workspace();
    let ws_config_file1 = ws_dir.join("jlf.toml");
    let ws_config_file2 = ws_dir.join(".jlf.toml");
    if ws_config_file1.exists() {
        let config_raw = fs::read_to_string(ws_config_file1)?;
        let ws_config = toml::from_str(&config_raw)?;
    } else if ws_config_file2.exists() {
        let config_raw = fs::read_to_string(ws_config_file2)?;
        let ws_config = toml::from_str(&config_raw)?;
    }

    Ok(config)
}

fn config_dir() -> PathBuf {
    // TODO: allow env var override
    let strategy = choose_base_strategy().expect("Unable to find the config directory!");
    let mut path = strategy.config_dir();
    path.push("jlf");
    path
}

/// This function starts searching the FS upward from the CWD
/// and returns the first directory that contains either `.git`, `.svn`, `.jj`
/// If no workspace was found returns (CWD, true).
/// Otherwise (workspace, false) is returned
fn find_workspace() -> PathBuf {
    let current_dir = current_working_dir();
    for ancestor in current_dir.ancestors() {
        if ancestor.join(".git").exists()
            || ancestor.join("jlf.toml").exists()
            || ancestor.join(".jlf.toml").exists()
            || ancestor.join(".svn").exists()
            || ancestor.join(".jj").exists()
        {
            return ancestor.to_owned();
        }
    }

    current_dir
}

// Get the current working directory.
// This information is managed internally as the call to std::env::current_dir
// might fail if the cwd has been deleted.
fn current_working_dir() -> PathBuf {
    // implementation of crossplatform pwd -L
    // we want pwd -L so that symlinked directories are handled correctly
    let mut cwd = std::env::current_dir().expect("Couldn't determine current working directory");

    let pwd = std::env::var_os("PWD");
    #[cfg(windows)]
    let pwd = pwd.or_else(|| std::env::var_os("CD"));

    if let Some(pwd) = pwd.map(PathBuf::from) {
        if pwd.canonicalize().ok().as_ref() == Some(&cwd) {
            cwd = pwd;
        }
    }

    cwd
}
