//! End-to-end per-record rendering: parse a log line, then render it through
//! the template a recipe produces. This is the actual runtime cost of a recipe
//! — recipe *resolution* happens once at startup, but the template it yields is
//! rendered for every record, so this is what determines throughput.
//!
//! Each benchmark renders the same three tracing-style records through a
//! different template, so you can compare a light recipe against the heavier
//! default (which colors the level via `$match` and dumps the rest as JSON).

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use jlf_core::{column, default_variables, expanded_format, Formatter, Json};

const INPUTS: &[&str] = &[
    r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"name":"my_api1"},{"name":"something"}]}"#,
    r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"name":"my_api2"},{"name":"something"},{"name":"some other value"}]}"#,
    r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"ERROR","message":"This is an error log","target":"my_service::my_module::my_api3","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"}]}"#,
];

/// Parse and render every input through `fmt`, reusing one scratch buffer — the
/// shape of the CLI's per-record loop.
fn render(fmt: &Formatter, scratch: &mut String) {
    let mut json = Json::Null;
    for line in INPUTS {
        scratch.clear();
        json.parse_replace(line).unwrap();
        let _ = fmt.as_log(&json).write_fmt(scratch);
    }
}

fn render_bench(c: &mut Criterion) {
    let vars = default_variables();
    // The full default `output` recipe: timestamp + colored `$match` level +
    // message + a JSON dump of the rest.
    let default_tpl = expanded_format("${@output}", &vars);
    // Just the `$match` level dispatch.
    let level_tpl = expanded_format("${@level}", &vars);
    // The lightest useful template: a single fallback-chain field.
    let message_tpl = "${?message|msg|fields.message}".to_owned();
    // A CSV row — exercises `$cols`/`$rows` repetition over fixed columns.
    let csv_tpl = "$cols( $key ),*\n$rows( $cols( ${value:csv} ),* )*";
    let csv_cols =
        vec![column("timestamp"), column("level"), column("message")];

    let mut group = c.benchmark_group("render (3 records)");

    let cases: &[(&str, &str, bool, bool)] = &[
        // name, template, no_color, compact
        ("default (colored)", &default_tpl, false, false),
        ("default (plain)", &default_tpl, true, false),
        ("default (compact)", &default_tpl, true, true),
        ("level $match", &level_tpl, true, false),
        ("message only", &message_tpl, true, false),
    ];
    for &(name, tpl, no_color, compact) in cases {
        let fmt = Formatter::new(tpl, no_color, compact).unwrap();
        let mut scratch = String::new();
        group.bench_function(name, |b| {
            b.iter(|| render(black_box(&fmt), &mut scratch))
        });
    }

    // CSV needs a column list for `$cols` to iterate.
    let csv_fmt = Formatter::new(csv_tpl, true, true)
        .unwrap()
        .with_columns(csv_cols);
    let mut scratch = String::new();
    group.bench_function("csv row", |b| {
        b.iter(|| render(black_box(&csv_fmt), &mut scratch))
    });

    group.finish();
}

criterion_group!(benches, render_bench);
criterion_main!(benches);
