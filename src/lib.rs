use std::io::{self, BufRead, IsTerminal, Write};

use clap::{Parser, Subcommand};
use owo_colors::OwoColorize;

pub mod colors;

mod json;
pub use json::{parse_json, Json, ParseError};

mod format;
pub use format::{parse_formatter, Formatter};

mod expand;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[command(flatten)]
    format: FormatArgs,

    /// Disable color output. If output is not a terminal, this is always true
    #[arg(short = 'n', long = "no-color", default_value_t = false)]
    no_color: bool,

    /// Display log with data in a compact format
    #[arg(short = 'c', long = "compact", default_value_t = false)]
    compact: bool,

    /// If log line is not valid JSON, then report it and exit, instead of
    /// printing the line as is
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,

    /// Take only the first N lines
    #[arg(short = 't', long = "take")]
    take: Option<usize>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the format string with variables expanded
    Expand {
        #[command(flatten)]
        format: FormatArgs,
    },
    /// List all variables
    List {
        /// Pass variable as key=value format; can be passed multiple times.
        #[arg(short = 'v', long = "variable")]
        variables: Option<Vec<String>>,
    },
}

#[derive(Debug, clap::Args)]
struct FormatArgs {
    #[arg(default_value = r#"{&output}"#)]
    format_string: String,
    /// Pass variable as key=value format; can be passed multiple times.
    #[arg(short = 'v', long = "variable")]
    variables: Option<Vec<String>>,
}

pub fn run() -> Result<(), color_eyre::Report> {
    color_eyre::install()?;

    let Args {
        format,
        no_color,
        compact,
        strict,
        take,
        command,
    } = Args::parse();

    if let Some(command) = command {
        match command {
            Command::Expand {
                format:
                    FormatArgs {
                        format_string,
                        variables,
                    },
            } => {
                let variables = get_variables(variables);

                println!("{}", expand::ExpandedFormat(format_string, variables));
            }
            Command::List { variables } => {
                let variables = get_variables(variables);
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

        let variables = get_variables(format.variables);
        let formatter = parse_formatter(&format.format_string, no_color, compact, &variables)?;

        let mut buf = stdin.lock();
        let mut line = String::new();
        let mut stripped;
        let mut json = Json::default();
        let mut taken = 0;

        while buf.read_line(&mut line)? != 0 {
            stripped = strip_ansi_escapes::strip_str(&line);

            // keep reference to bypass borrow checker
            // this is safe because we know that line always exists.
            // And, when the line gets cleared, the str slices in json are no
            // longer used.
            let line_ref = unsafe { &*(&stripped as *const String) };

            if let Err(e) = json.parse_replace(line_ref) {
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

                line.clear();
                continue;
            }

            let fmt = formatter.with_json(&json);
            stdout.write_fmt(format_args!("{fmt}\n"))?;

            // clear line to avoid appending
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

fn get_variables(args: Option<Vec<String>>) -> Vec<(String, String)> {
    let mut variables = vec![
        (
            "output".to_owned(),
            r#"{#key &log_fields}{&log}{#config compact} {:else}\n{/config}{/key}{&data_log}"#
                .to_owned(),
        ),
        (
            "log_fields".to_owned(),
            "{&timestamp|&level|&message}".to_owned(),
        ),
        (
            "log".to_owned(),
            "{&timestamp_log}{&level_log}{&message_log}".to_owned(),
        ),
        (
            "timestamp_log".to_owned(),
            "{#key &timestamp}{&timestamp:dimmed} {/key}".to_owned(),
        ),
        ("timestamp".to_owned(), "{timestamp}".to_owned()),
        (
            "level_log".to_owned(),
            "{#key &level}{&level:level} {/key}".to_owned(),
        ),
        ("level".to_owned(), "{level|lvl|severity}".to_owned()),
        ("message_log".to_owned(), "{&message}".to_owned()),
        (
            "message".to_owned(),
            "{message|msg|body|fields.message}".to_owned(),
        ),
        ("data_log".to_owned(), "{&data:json}".to_owned()),
        ("data".to_owned(), "{spans|data|}".to_owned()),
    ];

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
