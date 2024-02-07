use criterion::{black_box, criterion_group, criterion_main, Criterion};
use jl::{parse_json, Json};
use serde_json::Value;

fn custom_parse<'a>(value: &mut Json<'a>, inputs: &'a [&'a str]) {
    for input in inputs {
        value.parse_replace(input).unwrap();
    }
}

fn serde_parse(value: &mut Value, inputs: &[&str]) {
    for input in inputs {
        *value = serde_json::from_str(input).unwrap();
    }
}

fn custom_parse_bench(c: &mut Criterion) {
    let inputs = [
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"ERROR","message":"This is an error log","target":"my_service::my_module::my_api3","filename":"my-service/src/my_module.rs","line_number":78,"spans":[]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"ERROR","message":"This is an error log","target":"my_service::my_module::my_api3","filename":"my-service/src/my_module.rs","line_number":78,"spans":[]}"#,
    ];

    let mut value = parse_json(&inputs[0]).unwrap();

    c.bench_function("custom parse", |b| {
        b.iter(|| custom_parse(black_box(&mut value), black_box(&inputs)))
    });
}

fn serde_parse_bench(c: &mut Criterion) {
    let inputs = [
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-06T23:52:48.349676Z","level":"INFO","message":"This is a info log","target":"my_service::my_module::my_api","filename":"my-service/src/my_module.rs","line_number":77,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api1"},{"field2":"value2","field3":"value3","field4":1234,"field5":false}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"DEBUG","message":"This is a debug log","target":"my_service::my_module::my_api2","filename":"my-service/src/my_module.rs","line_number":78,"spans":[{"name":"my_service"},{"name":"my_func"},{"field1":"value1","name":"my_api2"},{"field2":"value2","field6":"value4","field4":1234,"field5":false},{"field7":123.123,"field8":"some other value"}]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"ERROR","message":"This is an error log","target":"my_service::my_module::my_api3","filename":"my-service/src/my_module.rs","line_number":78,"spans":[]}"#,
        r#"{"timestamp":"2024-02-07T23:52:48.349676Z","level":"ERROR","message":"This is an error log","target":"my_service::my_module::my_api3","filename":"my-service/src/my_module.rs","line_number":78,"spans":[]}"#,
    ];

    let mut value = Value::Null;

    c.bench_function("serde parse", |b| {
        b.iter(|| serde_parse(black_box(&mut value), black_box(&inputs)))
    });
}

criterion_group!(custom_benches, custom_parse_bench);
criterion_group!(serde_benches, serde_parse_bench);
criterion_main!(custom_benches, serde_benches);
