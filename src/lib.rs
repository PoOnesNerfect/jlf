use std::io::{self, BufRead, IsTerminal, Write};

use clap::{Parser, Subcommand};
use config::ConfigFile;
use owo_colors::OwoColorize;

pub mod colors;

mod json;
pub use json::{parse_json, Json, ParseError};

mod format;
pub use format::{FormattedLog, Formatter};

mod config;
mod expand;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Formatter to use to format json log. [default: {&output}]
    format: Option<String>,

    #[command(flatten)]
    variables: Variables,

    /// Disable color output. If output is not a terminal, this is always true.
    #[arg(short = 'n', long = "no-color", default_value_t = false)]
    no_color: bool,

    /// Display log in a compact format.
    #[arg(short = 'c', long = "compact", default_value_t = false)]
    compact: bool,

    /// If log line is not valid JSON, then report it and exit, instead of
    /// printing the line as is.
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,

    /// Take only the first N lines.
    #[arg(short = 't', long = "take")]
    take: Option<usize>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print variable with its inner variables expanded.
    /// If no variable is specified, the default format string will be used.
    Expand {
        /// Variable to expand
        variable: Option<String>,

        #[command(flatten)]
        variables: Variables,
    },
    /// List all variables
    List {
        #[command(flatten)]
        variables: Variables,
    },
}

#[derive(Debug, clap::Args)]
struct Variables {
    /// Pass variable as KEY=VALUE format; can be passed multiple times.
    #[arg(short = 'v', long = "variable", value_name = "KEY=VALUE")]
    variables: Option<Vec<String>>,
}

pub fn run() -> Result<(), color_eyre::Report> {
    color_eyre::install()?;

    let Args {
        format,
        variables,
        no_color,
        compact,
        strict,
        take,
        command,
    } = Args::parse();

    let ConfigFile {
        mut config,
        variables: config_variables,
    } = config::get_config()?;
    if let Some(format) = format {
        config.format = Some(format);
    }
    if compact {
        config.compact = Some(true);
    }
    if no_color {
        config.no_color = Some(true);
    }
    if strict {
        config.strict = Some(true);
    }
    let format = config.format.unwrap_or_else(|| "{&output}".to_owned());
    let compact = config.compact.unwrap_or(false);
    let no_color = config.no_color.unwrap_or(false);
    let strict = config.strict.unwrap_or(false);

    if let Some(command) = command {
        match command {
            Command::Expand {
                variable,
                variables: Variables { variables },
            } => {
                let variables = get_variables(config_variables, variables);
                let format = variable.map(|e| format!("{{&{e}}}")).unwrap_or(format);

                println!("{}", expand::expanded_format(&format, &variables));
            }
            Command::List { variables } => {
                let variables = get_variables(config_variables, variables.variables);
                let width = variables.iter().map(|(k, _)| k.len()).max().unwrap();
                for (k, v) in variables {
                    println!("{:width$} = {v}", k.bold(), width = width);
                }
            }
        }

        return Ok(());
    }

    let stdin = io::stdin();
    if !stdin.is_terminal() {
        let mut stdout = io::stdout().lock();

        let no_color = no_color || !stdout.is_terminal();

        let variables = get_variables(config_variables, variables.variables);
        let expanded = expand::expanded_format(&format, &variables);
        let formatter = Formatter::new(&expanded, no_color, compact)?;

        let mut buf = stdin.lock();

        // input line red from stdin
        let mut line = String::new();

        // ansi stripped line
        let mut stripped;

        // json data to use and re-use
        let mut json = Json::Null;

        // how many lines have we taken?
        let mut taken = 0;

        while buf.read_line(&mut line)? != 0 {
            stripped = strip_ansi_escapes::strip_str(&line);

            // keep reference to bypass borrow checker
            // this is safe because we know that line always exists.
            // And, when the line gets cleared, the str slices in json are no
            // longer used.
            let line_ref = unsafe { &*(&stripped as *const String) };

            if !line_ref.trim().is_empty() {
                match json.parse_replace(line_ref) {
                    Ok(()) => {
                        let log = formatter.as_log(&json);
                        writeln!(stdout, "{log}")?;
                    }
                    Err(e) => {
                        if strict {
                            if no_color {
                                stdout.write_fmt(format_args!("{:?}\n", e))?;
                            } else {
                                stdout.write_fmt(format_args!("{:?}\n", e.red()))?;
                            }
                            return Ok(());
                        }

                        if no_color {
                            stdout.write_fmt(format_args!("{stripped}"))?;
                        } else {
                            stdout.write_fmt(format_args!("{line}"))?;
                        }
                    }
                }
            }

            line.clear();

            // take only N lines if specified
            if let Some(take) = take.as_ref() {
                taken += 1;
                if taken >= *take {
                    break;
                }
            }
        }
    }

    Ok(())
}

fn get_variables(
    from_config: Option<Vec<(String, String)>>,
    args: Option<Vec<String>>,
) -> Vec<(String, String)> {
    let mut variables = vec![
        (
            "output".to_owned(),
            r#"{#key &log}{&log_fmt}{&new_line}{/key}{&data_fmt}"#.to_owned(),
        ),
        ("log".to_owned(), "{&timestamp|&level|&message}".to_owned()),
        (
            "log_fmt".to_owned(),
            "{&timestamp_fmt}{&level_fmt}{&message_fmt}".to_owned(),
        ),
        (
            "timestamp_fmt".to_owned(),
            "{#key &timestamp}{&timestamp:dimmed} {/key}".to_owned(),
        ),
        ("timestamp".to_owned(), "{timestamp}".to_owned()),
        (
            "level_fmt".to_owned(),
            "{#key &level}{&level:level} {/key}".to_owned(),
        ),
        ("level".to_owned(), "{level|lvl|severity}".to_owned()),
        ("message_fmt".to_owned(), "{&message}".to_owned()),
        (
            "message".to_owned(),
            "{message|msg|body|fields.message}".to_owned(),
        ),
        (
            "new_line".to_owned(),
            r#"{#key &data}{#config compact} {:else}\n{/config}{/key}"#.to_owned(),
        ),
        ("data_fmt".to_owned(), "{&data:json}".to_owned()),
        ("data".to_owned(), "{..}".to_owned()),
    ];

    if let Some(from_config) = from_config {
        for (k2, v2) in from_config {
            let v = variables
                .iter_mut()
                .find_map(|(k, v)| (k == &k2).then_some(v));

            if let Some(v) = v {
                *v = v2;
            } else {
                variables.push((k2, v2));
            }
        }
    }

    if let Some(args) = args {
        for (key, val) in args.iter().filter_map(|e| e.split_once('=')) {
            let v_mut = variables
                .iter_mut()
                .find_map(|(k, v)| (k == key).then_some(v));
            if let Some(v) = v_mut {
                *v = val.to_owned();
            } else {
                variables.push((key.to_owned(), val.to_owned()));
            }
        }
    }

    variables
}
