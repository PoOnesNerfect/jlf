//! The command being assembled. A `Builder` holds the user's choices and knows
//! how to turn them into (a) `jlf` CLI arguments for preview/run and (b) a
//! `[recipe.NAME]` TOML block for saving.

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// Pretty-print / project fields with a template.
    View,
    /// Aggregate with count/stats/top/uniq.
    Summarize,
    /// Emit a column table (csv/tsv/md).
    Export,
}

#[derive(Default, Clone)]
pub struct Builder {
    pub filters: Vec<String>,
    // View
    pub template: Option<String>,
    pub fields: Vec<String>,
    pub compact: bool,
    pub redact: Vec<String>,
    // Summarize
    pub verb: Option<String>, // count | stats | top | uniq
    pub field: Option<String>,
    pub by: Option<String>,
    pub n: Option<usize>,
    // Output format (export, or table output for a summary)
    pub format: Option<String>, // csv | tsv | md
}

impl Builder {
    pub fn mode(&self) -> Mode {
        if self.verb.is_some() {
            Mode::Summarize
        } else if self.format.is_some() && self.verb.is_none() {
            Mode::Export
        } else {
            Mode::View
        }
    }

    /// The `jlf` CLI arguments this builder represents (for preview / run).
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        match self.mode() {
            Mode::Summarize => {
                // subcommand first, then field / by / n, then filters, then
                // format
                args.push(self.verb.clone().unwrap());
                if let Some(f) = &self.field {
                    args.push(f.clone());
                }
                if let Some(by) = &self.by {
                    args.push("by".into());
                    args.push(by.clone());
                }
                if self.verb.as_deref() == Some("top") {
                    if let Some(n) = self.n {
                        args.push(n.to_string());
                    }
                }
                args.extend(self.filters.iter().cloned());
                self.push_format(&mut args);
            }
            Mode::Export => {
                self.push_format(&mut args);
                if !self.fields.is_empty() {
                    args.push(self.fields.join(","));
                }
                args.extend(self.filters.iter().cloned());
            }
            Mode::View => {
                args.extend(self.filters.iter().cloned());
                if let Some(t) = &self.template {
                    args.push(t.clone());
                } else if !self.fields.is_empty() {
                    args.push("-f".into());
                    args.push(self.fields.join(","));
                }
                if self.compact {
                    args.push("-c".into());
                }
                if !self.redact.is_empty() {
                    args.push("-r".into());
                    args.push(self.redact.join(","));
                }
            }
        }
        args
    }

    fn push_format(&self, args: &mut Vec<String>) {
        if let Some(fmt) = self.format.as_deref() {
            args.push(format!("@{fmt}"));
        }
    }

    /// A `[recipe.NAME]` TOML block equivalent to this builder.
    pub fn to_recipe_toml(&self, name: &str) -> String {
        let mut lines = vec![format!("[recipe.{name}]")];
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        macro_rules! kv {
            ($k:expr, $v:expr) => {
                lines.push(format!("{} = \"{}\"", $k, esc($v)))
            };
        }

        if !self.filters.is_empty() {
            kv!("filter", &self.filters.join(" "));
        }
        match self.mode() {
            Mode::Summarize => {
                let verb = self.verb.clone().unwrap();
                kv!(&verb, self.field.as_deref().unwrap_or(""));
                if let Some(by) = &self.by {
                    kv!("by", by);
                }
                if verb == "top" {
                    if let Some(n) = self.n {
                        lines.push(format!("n = {n}"));
                    }
                }
                if let Some(f) = &self.format {
                    kv!("format", f);
                }
            }
            Mode::Export => {
                if let Some(f) = &self.format {
                    kv!("format", f);
                }
                if !self.fields.is_empty() {
                    kv!("out", &self.fields.join(","));
                }
            }
            Mode::View => {
                if let Some(t) = &self.template {
                    kv!("out", t);
                } else if !self.fields.is_empty() {
                    kv!("out", &self.fields.join(","));
                }
                if self.compact {
                    lines.push("compact = true".into());
                }
                if !self.redact.is_empty() {
                    kv!("redact", &self.redact.join(","));
                }
            }
        }
        lines.push(String::new());
        lines.join("\n")
    }

    /// A shell-ish rendering of the equivalent command, for display.
    pub fn command_line(&self) -> String {
        let mut out = String::from("jlf");
        for a in self.to_args() {
            // Quote anything the shell would interpret (templates, filters, …).
            let special = [
                ' ', '{', '}', '|', '"', '$', '(', ')', '*', '?', '<', '>',
                '~', '!', '&', ';',
            ];
            if a.is_empty() || a.contains(special) {
                out.push_str(&format!(" '{a}'"));
            } else {
                out.push(' ');
                out.push_str(&a);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_with_filter_and_template() {
        let b = Builder {
            filters: vec!["level=error".into()],
            template: Some("${ts} ${msg}".into()),
            ..Default::default()
        };
        assert_eq!(b.to_args(), vec!["level=error", "${ts} ${msg}"]);
        assert!(b
            .to_recipe_toml("errs")
            .contains("filter = \"level=error\""));
        assert!(b.to_recipe_toml("errs").contains("out = \"${ts} ${msg}\""));
    }

    #[test]
    fn view_with_fields_compact_redact() {
        let b = Builder {
            fields: vec!["ts".into(), "level".into()],
            compact: true,
            redact: vec!["token".into()],
            ..Default::default()
        };
        assert_eq!(b.to_args(), vec!["-f", "ts,level", "-c", "-r", "token"]);
    }

    #[test]
    fn summarize_stats_by() {
        let b = Builder {
            verb: Some("stats".into()),
            field: Some("latency_ms".into()),
            by: Some("level".into()),
            format: Some("md".into()),
            ..Default::default()
        };
        assert_eq!(b.to_args(), vec![
            "stats",
            "latency_ms",
            "by",
            "level",
            "@md"
        ]);
        let toml = b.to_recipe_toml("lat");
        assert!(toml.contains("stats = \"latency_ms\""));
        assert!(toml.contains("by = \"level\""));
        assert!(toml.contains("format = \"md\""));
    }

    #[test]
    fn top_includes_n() {
        let b = Builder {
            verb: Some("top".into()),
            field: Some("user".into()),
            n: Some(5),
            ..Default::default()
        };
        assert_eq!(b.to_args(), vec!["top", "user", "5"]);
        assert!(b.to_recipe_toml("t").contains("n = 5"));
    }

    #[test]
    fn export_csv_columns() {
        let b = Builder {
            format: Some("csv".into()),
            fields: vec!["ts".into(), "level".into()],
            filters: vec!["level=error".into()],
            ..Default::default()
        };
        assert_eq!(b.to_args(), vec!["@csv", "ts,level", "level=error"]);
        let toml = b.to_recipe_toml("x");
        assert!(toml.contains("format = \"csv\""));
        assert!(toml.contains("out = \"ts,level\""));
    }
}
