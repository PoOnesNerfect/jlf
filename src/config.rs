use std::{fmt, fs, path::PathBuf};

use etcetera::{choose_base_strategy, BaseStrategy};
use serde::Deserialize;

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
    let ws_config = if ws_config_file1.exists() {
        let config_raw = fs::read_to_string(ws_config_file1)?;
        Some(toml::from_str(&config_raw)?)
    } else if ws_config_file2.exists() {
        let config_raw = fs::read_to_string(ws_config_file2)?;
        Some(toml::from_str(&config_raw)?)
    } else {
        None
    };

    if let Some(ws_config) = ws_config {
        config.merge(ws_config);
    }

    Ok(config)
}

#[derive(Debug, Default, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub config: Config,
    #[serde(default, deserialize_with = "de_map_to_list")]
    pub variables: Option<Vec<(String, String)>>,
}

fn de_map_to_list<'de, D>(de: D) -> Result<Option<Vec<(String, String)>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Option<Vec<(String, String)>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "A hex encoded OpId")
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_map(Visitor)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut list = map.size_hint().map(Vec::with_capacity).unwrap_or_default();

            while let Some((k, v)) = map.next_entry()? {
                list.push((k, v));
            }

            Ok(Some(list))
        }
    }

    de.deserialize_any(Visitor)
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub format: Option<String>,
    pub compact: Option<bool>,
    pub no_color: Option<bool>,
    pub strict: Option<bool>,
}

impl ConfigFile {
    fn merge(&mut self, other: Self) {
        let Self { config, variables } = self;
        let Self {
            config: config2,
            variables: variables2,
        } = other;

        if let Some(format) = config2.format {
            config.format = Some(format);
        }
        if let Some(compact) = config2.compact {
            config.compact = Some(compact);
        }
        if let Some(no_color) = config2.no_color {
            config.no_color = Some(no_color);
        }
        if let Some(strict) = config2.strict {
            config.strict = Some(strict);
        }

        match (variables, variables2) {
            (_, None) => (),
            (v1, Some(v2)) => {
                if let Some(v1) = v1 {
                    for (k2, v2) in v2 {
                        let v = v1.iter_mut().find_map(|(k, v)| (k == &k2).then_some(v));

                        if let Some(v) = v {
                            *v = v2;
                        } else {
                            v1.push((k2, v2));
                        }
                    }
                } else {
                    *v1 = Some(v2);
                }
            }
        }
    }
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
