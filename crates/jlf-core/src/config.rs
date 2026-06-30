use std::collections::HashMap;
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

/// A unified recipe: one named, reusable definition that can act as a template
/// fragment (`{@name}`), a saved command (`@name` / `-p`), a named field, or a
/// custom output format. See docs/RECIPES.md. Recipes translate down to the
/// existing variable/preset/format machinery.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct Recipe {
    /// Per-record layout (a template string).
    pub body: Option<String>,
    /// Shorthand for a `{a} {b} {c}` body.
    pub fields: Option<String>,
    /// Value accessor — makes the recipe usable as a value (`{@name}`, filters).
    pub field: Option<String>,
    /// Render modifier(s) for `field` (e.g. `level`, `dimmed`, `json`).
    pub style: Option<String>,
    /// Records to keep (`key=value`, space-separated).
    pub filter: Option<String>,
    pub redact: Option<String>,
    /// Reference an output format (built-in `csv`/`tsv`/`md` or another recipe).
    pub format: Option<String>,
    pub header: Option<String>,
    pub footer: Option<String>,
    pub escape: Option<String>,
    pub count: Option<String>,
    pub stats: Option<String>,
    pub top: Option<String>,
    pub uniq: Option<String>,
    pub by: Option<String>,
    pub n: Option<usize>,
    /// Inherit another recipe (`@other` or `other`), then override its keys.
    pub base: Option<String>,
    /// Conditional overrides from `[recipe.NAME.<cond>]` sub-tables: when the
    /// condition holds (a config flag like `compact`), these keys override.
    #[serde(flatten, default)]
    pub overrides: HashMap<String, Recipe>,
}

impl Recipe {
    /// The string used when this recipe is inlined (`{@name}`) or used as a
    /// render template: `body`, else `{field[:style]}`, else a `{a} {b}` from
    /// `fields`, else empty.
    fn inline_body(&self) -> Option<String> {
        if let Some(body) = &self.body {
            Some(body.clone())
        } else if let Some(field) = &self.field {
            Some(match &self.style {
                Some(style) => format!("{{{field}:{style}}}"),
                None => format!("{{{field}}}"),
            })
        } else if let Some(fields) = &self.fields {
            Some(
                fields
                    .split(',')
                    .map(|f| format!("{{{}}}", f.trim()))
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        } else {
            None
        }
    }

    /// Shallow-merge `other`'s set keys over `self` (used for base inheritance
    /// and conditional overrides). `self` is the lower-priority base.
    fn overlay(&mut self, other: &Recipe) {
        macro_rules! take {
            ($($f:ident),*) => { $( if other.$f.is_some() { self.$f = other.$f.clone(); } )* };
        }
        take!(body, fields, field, style, filter, redact, format, header, footer,
              escape, count, stats, top, uniq, by, n, base);
    }

    /// Does this recipe describe a custom output format (header/footer/escape)?
    fn is_format(&self) -> bool {
        self.header.is_some() || self.footer.is_some() || self.escape.is_some()
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub config: Config,
    #[serde(default, deserialize_with = "de_map_to_list")]
    pub variables: Option<Vec<(String, String)>>,
    /// User-defined output formats from `[format.NAME]` tables.
    #[serde(default, rename = "format")]
    pub formats: HashMap<String, FormatDef>,
    /// Saved argument bundles from `[preset.NAME]` tables.
    #[serde(default, rename = "preset")]
    pub presets: HashMap<String, PresetDef>,
    /// Body-only recipe shorthand: `[recipes]` maps name -> body string.
    #[serde(default)]
    pub recipes: HashMap<String, String>,
    /// Full recipes from `[recipe.NAME]` tables.
    #[serde(default, rename = "recipe")]
    pub recipe: HashMap<String, Recipe>,
}

/// A custom output format: `header`/`footer` printed once, `row` rendered per
/// record, with interpolated values escaped per `escape`.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct FormatDef {
    pub escape: Option<String>,
    pub header: Option<String>,
    pub row: String,
    pub footer: Option<String>,
}

/// A saved bundle of arguments invoked by name (`@name` / `-p name`). Keys
/// mirror the explicit flag forms; explicit CLI args layer on top.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct PresetDef {
    /// Filters, as a single `key=value ...` string (space-separated).
    #[serde(rename = "where")]
    pub filter: Option<String>,
    pub template: Option<String>,
    pub fields: Option<String>,
    pub redact: Option<String>,
    pub compact: Option<bool>,
    pub format: Option<String>,
    // summary verbs: at most one is set
    pub count: Option<String>,
    pub stats: Option<String>,
    pub top: Option<String>,
    pub uniq: Option<String>,
    pub by: Option<String>,
    pub n: Option<usize>,
}

/// The built-in default template variables, used when no config overrides them.
/// Shared by the CLI and the TUI so both render records identically.
///
/// The recipe-style form (docs/RECIPES.md): `output` joins reusable field
/// variables; each field is optional (`{?…}`) so an absent one collapses its
/// space, and the separator before the JSON is chosen by the `compact` flag.
/// Overriding any field variable (e.g. `-v level=…`) still recolors the output.
pub fn default_variables() -> Vec<(String, String)> {
    [
        (
            "output",
            "{&timestamp}{&level}{&message}{#config compact} {:else}\\n{/config}{&data}",
        ),
        ("timestamp", "{?timestamp:dimmed} "),
        ("level", "{?lvl|level|severity:level} "),
        ("message", "{?message|msg|body|fields.message}"),
        ("data", "{?..:json}"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
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
        let Self {
            config: config2,
            variables: variables2,
            formats: formats2,
            presets: presets2,
            recipes: recipes2,
            recipe: recipe2,
        } = other;

        if let Some(format) = config2.format {
            self.config.format = Some(format);
        }
        if let Some(compact) = config2.compact {
            self.config.compact = Some(compact);
        }
        if let Some(no_color) = config2.no_color {
            self.config.no_color = Some(no_color);
        }
        if let Some(strict) = config2.strict {
            self.config.strict = Some(strict);
        }

        // Workspace-defined formats/presets/recipes override same-named base ones.
        self.formats.extend(formats2);
        self.presets.extend(presets2);
        self.recipes.extend(recipes2);
        self.recipe.extend(recipe2);

        match (&mut self.variables, variables2) {
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

    /// Translate recipes (`[recipes]` + `[recipe.NAME]`) into the variable,
    /// preset, and format maps the rest of the engine consumes. `active` is the
    /// set of currently-true condition flags (e.g. `["compact"]`), used to apply
    /// `[recipe.NAME.<cond>]` overrides. Idempotent; call once flags are known.
    pub fn resolve_recipes(&mut self, active: &[&str]) {
        // Body-only shorthand: a variable, and a runnable preset (template=body).
        let shorthand: Vec<(String, String)> = self.recipes.drain().collect();
        for (name, body) in shorthand {
            self.set_variable(name.clone(), body.clone());
            self.presets
                .entry(name)
                .or_insert_with(|| PresetDef { template: Some(body), ..Default::default() });
        }

        let recipes: Vec<(String, Recipe)> = self.recipe.clone().into_iter().collect();
        for (name, _) in &recipes {
            let resolved = self.resolve_one(name, active, &mut Vec::new());
            self.install_recipe(name, &resolved);
        }
        self.recipe.clear();
    }

    /// Resolve a recipe by name: apply `base` inheritance (depth-first) then the
    /// active conditional overrides. `stack` guards against base cycles.
    fn resolve_one(&self, name: &str, active: &[&str], stack: &mut Vec<String>) -> Recipe {
        let Some(raw) = self.recipe.get(name) else {
            return Recipe::default();
        };
        if stack.iter().any(|n| n == name) {
            return raw.clone(); // cycle: stop inheriting
        }
        stack.push(name.to_owned());

        let mut resolved = Recipe::default();
        if let Some(base) = &raw.base {
            let base_name = base.strip_prefix('@').unwrap_or(base);
            resolved = self.resolve_one(base_name, active, stack);
        }
        resolved.overlay(raw);
        for cond in active {
            if let Some(ov) = raw.overrides.get(*cond) {
                resolved.overlay(ov);
            }
        }
        stack.pop();
        resolved
    }

    /// Register a resolved recipe as a variable (inline), a preset (run), and a
    /// format (if it frames output).
    fn install_recipe(&mut self, name: &str, r: &Recipe) {
        if let Some(body) = r.inline_body() {
            self.set_variable(name.to_owned(), body);
        }
        if r.is_format() {
            self.formats.insert(
                name.to_owned(),
                FormatDef {
                    escape: r.escape.clone(),
                    header: r.header.clone(),
                    row: r.inline_body().unwrap_or_default(),
                    footer: r.footer.clone(),
                },
            );
        }
        // Any recipe with content is runnable as a preset.
        let preset = PresetDef {
            filter: r.filter.clone(),
            template: r.body.clone().or_else(|| {
                r.field.as_ref().map(|f| match &r.style {
                    Some(s) => format!("{{{f}:{s}}}"),
                    None => format!("{{{f}}}"),
                })
            }),
            fields: r.fields.clone(),
            redact: r.redact.clone(),
            compact: None,
            format: r.format.clone().or_else(|| r.is_format().then(|| name.to_owned())),
            count: r.count.clone(),
            stats: r.stats.clone(),
            top: r.top.clone(),
            uniq: r.uniq.clone(),
            by: r.by.clone(),
            n: r.n,
        };
        self.presets.insert(name.to_owned(), preset);
    }

    fn set_variable(&mut self, key: String, value: String) {
        let vars = self.variables.get_or_insert_with(Vec::new);
        match vars.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = value,
            None => vars.push((key, value)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> ConfigFile {
        toml::from_str(s).unwrap()
    }

    fn var<'a>(c: &'a ConfigFile, name: &str) -> Option<&'a str> {
        c.variables
            .as_ref()?
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn shorthand_recipe_becomes_variable_and_preset() {
        let mut c = parse("[recipes]\noneline = \"{level} {msg}\"\n");
        c.resolve_recipes(&[]);
        assert_eq!(var(&c, "oneline"), Some("{level} {msg}"));
        assert_eq!(c.presets["oneline"].template.as_deref(), Some("{level} {msg}"));
    }

    #[test]
    fn field_style_recipe_becomes_field_variable() {
        let mut c = parse("[recipe.level]\nfield = \"lvl|level|severity\"\nstyle = \"level\"\n");
        c.resolve_recipes(&[]);
        assert_eq!(var(&c, "level"), Some("{lvl|level|severity:level}"));
    }

    #[test]
    fn filter_body_recipe_becomes_preset() {
        let mut c = parse(
            "[recipe.errors]\nfilter = \"level=error\"\nbody = \"{ts} {msg}\"\n",
        );
        c.resolve_recipes(&[]);
        let p = &c.presets["errors"];
        assert_eq!(p.filter.as_deref(), Some("level=error"));
        assert_eq!(p.template.as_deref(), Some("{ts} {msg}"));
    }

    #[test]
    fn format_recipe_becomes_format() {
        let mut c = parse(
            "[recipe.report]\nescape = \"html\"\nheader = \"<h>\"\nbody = \"<r>{msg}</r>\"\nfooter = \"<f>\"\n",
        );
        c.resolve_recipes(&[]);
        let f = &c.formats["report"];
        assert_eq!(f.escape.as_deref(), Some("html"));
        assert_eq!(f.row, "<r>{msg}</r>");
        // and it's runnable as a preset that points at its own format
        assert_eq!(c.presets["report"].format.as_deref(), Some("report"));
    }

    #[test]
    fn conditional_override_applies_when_active() {
        let toml = "[recipe.sep]\nbody = \"\\n\"\n[recipe.sep.compact]\nbody = \" \"\n";
        let mut normal = parse(toml);
        normal.resolve_recipes(&[]);
        assert_eq!(var(&normal, "sep"), Some("\n"));

        let mut compact = parse(toml);
        compact.resolve_recipes(&["compact"]);
        assert_eq!(var(&compact, "sep"), Some(" "));
    }

    #[test]
    fn base_inheritance_overlays_keys() {
        let mut c = parse(
            "[recipe.base]\nbody = \"{a}\"\nfilter = \"x=1\"\n[recipe.child]\nbase = \"@base\"\nbody = \"{b}\"\n",
        );
        c.resolve_recipes(&[]);
        // child overrides body, inherits filter
        assert_eq!(c.presets["child"].template.as_deref(), Some("{b}"));
        assert_eq!(c.presets["child"].filter.as_deref(), Some("x=1"));
    }
}
