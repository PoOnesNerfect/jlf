//! End-to-end tests that drive the built `jlf` binary the way a user would:
//! feed NDJSON on stdin (or via `-i`) and assert on stdout. Output is piped, so
//! `--color=auto` already strips color — no extra flag needed.

use std::io::Write;
use std::process::{Command, Stdio};

const SAMPLE: &str = r#"{"ts":"10:00:01","level":"info","msg":"login","user":"alice","latency_ms":42,"token":"abc123"}
{"ts":"10:00:02","level":"error","msg":"db timeout","user":"bob","latency_ms":510,"token":"xyz"}
{"ts":"10:00:03","level":"warn","msg":"retry","user":"alice","latency_ms":88}
{"ts":"10:00:04","level":"info","msg":"login","user":"carol","latency_ms":33,"token":"q9"}
{"ts":"10:00:05","level":"error","msg":"db timeout","user":"alice","latency_ms":620}
"#;

/// Run `jlf <args>` with `stdin` piped in; return (stdout, exit_code).
fn run(args: &[&str], stdin: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jlf"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn jlf");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8(out.stdout).unwrap(),
        out.status.code().unwrap_or(-1),
    )
}

fn stdout(args: &[&str]) -> String {
    run(args, SAMPLE).0
}

#[test]
fn template_projects_fields() {
    assert_eq!(
        stdout(&["{ts} {level} {user}"]),
        "10:00:01 info alice\n\
         10:00:02 error bob\n\
         10:00:03 warn alice\n\
         10:00:04 info carol\n\
         10:00:05 error alice\n"
    );
}

#[test]
fn fields_flag_is_a_template_shortcut() {
    assert_eq!(stdout(&["-f", "ts,level,user"]), stdout(&["{ts} {level} {user}"]));
}

#[test]
fn filter_keeps_matching_records() {
    assert_eq!(
        stdout(&["level=error", "{ts} {user}"]),
        "10:00:02 bob\n10:00:05 alice\n"
    );
}

#[test]
fn numeric_filter() {
    assert_eq!(
        stdout(&["latency_ms>100", "{user} {latency_ms}"]),
        "bob 510\nalice 620\n"
    );
}

#[test]
fn or_within_field_and_across_filters() {
    // (error OR warn) AND user=alice
    assert_eq!(stdout(&["level=error,warn", "user=alice", "{ts}"]), "10:00:03\n10:00:05\n");
}

#[test]
fn take_counts_emitted_records_after_filtering() {
    // Two errors exist; --take 1 must stop after the first emitted (filtered) one.
    assert_eq!(stdout(&["level=error", "--take", "1", "{ts}"]), "10:00:02\n");
}

#[test]
fn count_total() {
    assert_eq!(stdout(&["count"]), "5\n");
}

#[test]
fn count_by_field_has_header_and_total() {
    let out = stdout(&["count", "level"]);
    assert!(out.starts_with("     count  value\n"), "got:\n{out}");
    assert!(out.contains("         2  info\n"));
    assert!(out.contains("         2  error\n"));
    assert!(out.contains("         1  warn\n"));
    assert!(out.trim_end().ends_with("         5  total"));
}

#[test]
fn uniq_distinct_count() {
    assert_eq!(stdout(&["uniq", "user"]), "3 distinct (of 5 values)\n");
}

#[test]
fn stats_percentiles() {
    let out = stdout(&["stats", "latency_ms"]);
    assert!(out.contains("count 5\n"), "got:\n{out}");
    assert!(out.contains("min   33.00\n"));
    assert!(out.contains("max   620.00\n"));
    assert!(out.contains("p50   88.00\n"));
}

#[test]
fn top_with_share_and_footer() {
    let out = stdout(&["top", "user"]);
    assert!(out.starts_with("     count   share  value\n"), "got:\n{out}");
    assert!(out.contains("         3   60.0%  alice\n"));
    assert!(out.trim_end().ends_with("top 3 of 3 distinct (5 values)"));
}

#[test]
fn csv_export_with_header() {
    assert_eq!(
        stdout(&["--csv", "ts,level,user"]),
        "ts,level,user\n\
         10:00:01,info,alice\n\
         10:00:02,error,bob\n\
         10:00:03,warn,alice\n\
         10:00:04,info,carol\n\
         10:00:05,error,alice\n"
    );
}

#[test]
fn md_export_with_separator_row() {
    let out = stdout(&["--md", "level,user"]);
    assert!(out.starts_with("| level | user |\n| --- | --- |\n"), "got:\n{out}");
    assert!(out.contains("| info | alice |\n"));
}

#[test]
fn summary_export_to_csv_has_header() {
    let out = stdout(&["count", "level", "--csv"]);
    assert!(out.starts_with("count,value\n"), "got:\n{out}");
}

#[test]
fn redact_masks_named_field() {
    let out = stdout(&["-c", "-r", "token"]);
    assert!(out.contains(r#""token":"***""#), "got:\n{out}");
    assert!(!out.contains("abc123"), "secret leaked:\n{out}");
}

#[test]
fn strict_exits_nonzero_on_invalid_json() {
    let (_out, code) = run(&["-s"], "not json at all\n");
    assert_eq!(code, 1);
}

#[test]
fn non_strict_passes_through_invalid_lines() {
    let (out, code) = run(&[], "not json at all\n");
    assert_eq!(code, 0);
    assert_eq!(out, "not json at all\n");
}

#[test]
fn input_flag_reads_a_file_without_stdin() {
    let mut path = std::env::temp_dir();
    path.push(format!("jlf_test_{}.ndjson", std::process::id()));
    std::fs::write(&path, SAMPLE).unwrap();
    let p = path.to_str().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_jlf"))
        .args(["-i", p, "count"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "5\n");
}

/// Records used by the conditional tests: `body` is an empty string and
/// `data.count` is 0 — both present but falsey.
const COND: &str = "{\"message\":\"hi\",\"body\":\"\",\"data\":{\"count\":0}}\n";

#[test]
fn if_or_list_is_true_when_any_field_is_truthy() {
    // `body` (empty) and `data.count` (0) are present-but-falsey and must not
    // short-circuit the OR before reaching the truthy `message`.
    let (out, _) = run(&["{#if body|data.count|message}yes{:else}no{/if}"], COND);
    assert_eq!(out, "yes\n");
}

#[test]
fn if_single_falsey_field_is_false() {
    assert_eq!(run(&["{#if body}yes{:else}no{/if}"], COND).0, "no\n");
    assert_eq!(run(&["{#if data.count}yes{:else}no{/if}"], COND).0, "no\n");
}

#[test]
fn key_is_true_for_present_but_falsey_field() {
    // `#key` checks existence, so an empty string still counts.
    assert_eq!(run(&["{#key body}has{:else}missing{/key}"], COND).0, "has\n");
}

#[test]
fn key_or_list_falls_through_to_first_present() {
    assert_eq!(run(&["{#key msg}m{:else key message}message{/key}"], COND).0, "message\n");
}

/// `{?field}` collapses one adjacent space when the field is empty/absent, but
/// leaves plain `{field}` and intentional spacing untouched.
mod optional_field {
    use super::run;

    fn render(template: &str, json: &str) -> String {
        run(&[template], &format!("{json}\n")).0
    }

    #[test]
    fn collapses_one_space_in_the_middle_when_absent() {
        assert_eq!(render("{a} {?b} {c}", r#"{"a":"A","c":"C"}"#), "A C\n");
    }

    #[test]
    fn keeps_spacing_when_present() {
        assert_eq!(render("{a} {?b} {c}", r#"{"a":"A","b":"B","c":"C"}"#), "A B C\n");
    }

    #[test]
    fn trims_trailing_space_at_line_end() {
        assert_eq!(render("{a} {?b}", r#"{"a":"A"}"#), "A\n");
    }

    #[test]
    fn trims_leading_space_at_line_start() {
        assert_eq!(render("{?a} {b}", r#"{"b":"B"}"#), "B\n");
    }

    #[test]
    fn collapses_consecutive_absent_optionals() {
        assert_eq!(render("{a} {?b} {?c} {d}", r#"{"a":"A","d":"D"}"#), "A D\n");
    }

    #[test]
    fn empty_string_value_also_collapses() {
        assert_eq!(render("{a} {?b} {c}", r#"{"a":"A","b":"","c":"C"}"#), "A C\n");
    }

    #[test]
    fn plain_field_does_not_collapse() {
        // a missing plain {b} leaves the two surrounding spaces intact
        assert_eq!(render("{a} {b} {c}", r#"{"a":"A","c":"C"}"#), "A  C\n");
    }

    #[test]
    fn intentional_indentation_is_preserved() {
        assert_eq!(render("  {?label}: {v}", r#"{"label":"L","v":"V"}"#), "  L: V\n");
    }

    #[test]
    fn fallbacks_and_styles_work_on_optionals() {
        assert_eq!(render("{a} {?x.y|z} {c}", r#"{"a":"A","z":"Z","c":"C"}"#), "A Z C\n");
        assert_eq!(render("{a} {?x.y|z} {c}", r#"{"a":"A","c":"C"}"#), "A C\n");
    }
}

/// `[format.*]` custom output formats and the `--format` flag.
mod custom_format {
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn run_in(dir: &std::path::Path, args: &[&str], stdin: &str) -> String {
        let mut child = Command::new(env!("CARGO_BIN_EXE_jlf"))
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    }

    #[test]
    fn custom_html_format_escapes_values() {
        let dir = std::env::temp_dir().join(format!("jlf_fmt_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // `.git` marker so the dir is treated as the workspace root
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(
            dir.join("jlf.toml"),
            r#"
[format.report]
escape = "html"
header = "<table>\n"
row = "<tr><td>{level}</td><td>{msg}</td></tr>"
footer = "</table>\n"
"#,
        )
        .unwrap();

        let out = run_in(
            &dir,
            &["--format", "report"],
            "{\"level\":\"info\",\"msg\":\"a <b> & c\"}\n",
        );
        std::fs::remove_dir_all(&dir).ok();

        assert!(out.starts_with("<table>\n"), "got:\n{out}");
        assert!(out.contains("<tr><td>info</td><td>a &lt;b&gt; &amp; c</td></tr>\n"), "got:\n{out}");
        assert!(out.trim_end().ends_with("</table>"), "got:\n{out}");
    }

    #[test]
    fn format_csv_is_a_builtin_shorthand() {
        let dir = std::env::temp_dir().join(format!("jlf_fmt_csv_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = run_in(
            &dir,
            &["--format", "csv", "level,msg"],
            "{\"level\":\"info\",\"msg\":\"hi\"}\n",
        );
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(out, "level,msg\ninfo,hi\n");
    }
}

/// `[preset.*]` saved bundles invoked via `@name` / `-p`.
mod presets {
    use std::io::Write;
    use std::process::{Command, Stdio};

    const CONFIG: &str = r#"
[preset.errors]
where = "level=error,fatal"
template = "{level} {msg}"

[preset.lat]
where = "latency_ms>=100"
stats = "latency_ms"

[preset.acts]
top = "action"
n = 2
"#;

    const LOGS: &str = concat!(
        "{\"level\":\"info\",\"msg\":\"a\",\"latency_ms\":42,\"action\":\"x\"}\n",
        "{\"level\":\"error\",\"msg\":\"b\",\"latency_ms\":510,\"action\":\"y\"}\n",
        "{\"level\":\"warn\",\"msg\":\"c\",\"latency_ms\":88,\"action\":\"x\"}\n",
        "{\"level\":\"fatal\",\"msg\":\"d\",\"latency_ms\":900,\"action\":\"x\"}\n",
    );

    fn run_preset(args: &[&str]) -> String {
        let dir = std::env::temp_dir().join(format!(
            "jlf_preset_{}_{}",
            std::process::id(),
            args.join("_").replace(['@', '=', '/', ' '], "-")
        ));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("jlf.toml"), CONFIG).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_jlf"))
            .args(args)
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(LOGS.as_bytes()).unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        out
    }

    #[test]
    fn view_preset_applies_filter_and_template() {
        assert_eq!(run_preset(&["@errors"]), "error b\nfatal d\n");
    }

    #[test]
    fn explicit_filter_on_new_field_is_added() {
        // adds action=x on top of the preset's level filter
        assert_eq!(run_preset(&["@errors", "action=x"]), "fatal d\n");
    }

    #[test]
    fn explicit_filter_overrides_same_field() {
        // level=warn replaces the preset's level=error,fatal
        assert_eq!(run_preset(&["@errors", "level=warn"]), "warn c\n");
    }

    #[test]
    fn explicit_template_overrides_preset() {
        assert_eq!(run_preset(&["@errors", "M:{msg}"]), "M:b\nM:d\n");
    }

    #[test]
    fn summary_preset_runs_stats() {
        // only latency_ms >= 100 (510, 900)
        let out = run_preset(&["-p", "lat"]);
        assert!(out.contains("count 2"), "got:\n{out}");
        assert!(out.contains("max   900.00"), "got:\n{out}");
    }

    #[test]
    fn top_preset_respects_n() {
        let out = run_preset(&["@acts"]);
        assert!(out.starts_with("     count   share  value\n"), "got:\n{out}");
        assert!(out.contains("  x\n"));
        assert!(out.trim_end().ends_with("top 2 of 2 distinct (4 values)"), "got:\n{out}");
    }

    #[test]
    fn unknown_preset_errors() {
        // no stdout on error; just ensure it doesn't render records
        assert_eq!(run_preset(&["@missing"]), "");
    }
}

/// Phase 1 of the recipes redesign: `{@name}` aliases `{&name}`, and filters
/// accept `a|b|c` fallback fields.
mod recipes_phase1 {
    use super::run;

    #[test]
    fn at_sign_is_an_alias_for_ampersand_variable_include() {
        let json = "{\"timestamp\":\"T\",\"level\":\"INFO\",\"message\":\"hi\"}\n";
        let amp = run(&["-v", "output={&message}", "{&output}"], json).0;
        let at = run(&["-v", "output={@message}", "{@output}"], json).0;
        assert_eq!(at, amp);
        assert_eq!(at, "hi\n");
    }

    #[test]
    fn filter_fallback_fields() {
        let logs = concat!(
            "{\"lvl\":\"error\",\"msg\":\"a\"}\n",
            "{\"level\":\"info\",\"msg\":\"b\"}\n",
            "{\"severity\":\"error\",\"msg\":\"c\"}\n",
        );
        assert_eq!(run(&["lvl|level|severity=error", "{msg}"], logs).0, "a\nc\n");
    }
}

/// Phase 5 render rules: optional rest is empty when nothing's left over, and
/// the `?`-collapse absorbs an adjacent newline.
mod recipes_phase5 {
    use super::run;

    fn render(template: &str, json: &str) -> String {
        run(&[template], &format!("{json}\n")).0
    }

    #[test]
    fn optional_rest_is_empty_when_fully_consumed() {
        // was "X {}" before; the {?..} now collapses to nothing
        assert_eq!(render("{a} {?..:json}", r#"{"a":"X"}"#), "X\n");
    }

    #[test]
    fn optional_rest_renders_when_leftover_exists() {
        let out = render("{a} {?..:json}", r#"{"a":"X","b":"Y"}"#);
        assert!(out.starts_with("X {"), "got: {out:?}");
        assert!(out.contains("\"b\": \"Y\""), "got: {out:?}");
    }

    #[test]
    fn collapse_absorbs_a_newline_separator() {
        // newline before an empty optional is dropped
        assert_eq!(render("{a}\n{?..:json}", r#"{"a":"X"}"#), "X\n");
        // ...but stays when the optional renders
        let out = render("{a}\n{?..:json}", r#"{"a":"X","b":"Y"}"#);
        assert!(out.starts_with("X\n{"), "got: {out:?}");
    }

    #[test]
    fn plain_rest_still_prints_empty_object() {
        // only the `?` form collapses; plain {..} is unchanged
        assert_eq!(render("{a} {..:json}", r#"{"a":"X"}"#), "X {}\n");
    }
}
