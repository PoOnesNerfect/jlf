use criterion::{black_box, criterion_group, criterion_main, Criterion};
use json_logger::{parse_json, Json};
use serde::Deserialize;
use serde_json::Value;

const INPUTS: &'static [&'static str] = &[
    r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
    r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
    r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"ERROR","message":"This is an error log","target":"my_service::my_module::my_api3","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"}]}"#,
];

fn custom_parse<'a>(value: &mut Json<'a>, inputs: &'a [&'a str]) {
    for input in inputs {
        value.parse_replace(input).unwrap();
        // assert!(value
        //     .get("timestamp")
        //     .as_value()
        //     .unwrap()
        //     .starts_with("\"2024"));
        // assert!(value
        //     .get("spans")
        //     .get_i(0)
        //     .get("name")
        //     .as_value()
        //     .unwrap()
        //     .starts_with("\"my_service\""));
    }
}

fn serde_value_parse(value: &mut Value, inputs: &[&str]) {
    for input in inputs {
        *value = serde_json::from_str(input).unwrap();
        // assert!(value
        //     .get("timestamp")
        //     .unwrap()
        //     .as_str()
        //     .unwrap()
        //     .starts_with("2024"));
        // assert!(value
        //     .get("spans")
        //     .unwrap()
        //     .get(0)
        //     .unwrap()
        //     .get("name")
        //     .unwrap()
        //     .as_str()
        //     .unwrap()
        //     .starts_with("my_service"));
    }
}

#[derive(Deserialize)]
pub struct Log<'a> {
    timestamp: &'a str,
    level: &'a str,
    message: &'a str,
    target: &'a str,
    filename: &'a str,
    line_number: u32,
    spans: Vec<Value>,
}

fn serde_structured_parse<'a>(value: &mut Log<'a>, inputs: &[&'a str]) {
    for input in inputs {
        *value = serde_json::from_str(input).unwrap();
        // assert!(value.timestamp.starts_with("2024"));
        // assert!(value
        //     .spans
        //     .get(0)
        //     .unwrap()
        //     .get("name")
        //     .unwrap()
        //     .as_str()
        //     .unwrap()
        //     .starts_with("my_service"));
    }
}

fn custom_parse_bench(c: &mut Criterion) {
    let mut value = parse_json(&INPUTS[0]).unwrap();

    c.bench_function("custom parse", |b| {
        b.iter(|| custom_parse(black_box(&mut value), black_box(&INPUTS)))
    });
}

fn serde_value_parse_bench(c: &mut Criterion) {
    let mut value = Value::Null;

    c.bench_function("serde value parse", |b| {
        b.iter(|| serde_value_parse(black_box(&mut value), black_box(&INPUTS)))
    });
}

fn serde_structured_parse_bench(c: &mut Criterion) {
    let mut value = serde_json::from_str(&INPUTS[0]).unwrap();

    c.bench_function("serde structured parse", |b| {
        b.iter(|| serde_structured_parse(black_box(&mut value), black_box(&INPUTS)))
    });
}

criterion_group!(
    benches,
    custom_parse_bench,
    serde_value_parse_bench,
    serde_structured_parse_bench
);
criterion_main!(benches);
