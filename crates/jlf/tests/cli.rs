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
