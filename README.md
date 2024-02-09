# jlf

jlf is a simple cli for formatting json logs.

Given some log file as below:

[./examples/dummy_logs](https://github.com/PoOnesNerfect/jlf/blob/main/examples/dummy_logs)

```sh
{"timestamp": "2024-02-09T07:22:41.439284", "level": "DEBUG", "message": "User logged in successfully", "data": {"user_id": 3175, "session_id": "Nsb3P5mZ7971NFIt", "ip_address": "149.215.200.169", "action": "login", "success": false, "error_code": null}}
{"timestamp": "2024-02-09T07:22:42.439284", "level": "ERROR", "message": "Database connection established", "data": {"user_id": 8466, "session_id": "ZMOXKPna3GbzWz2N", "ip_address": "213.135.167.95", "action": "logout", "success": true, "error_code": null}}
...
```

You can format it using `jlf` as below:

```sh
cat ./examples/dummy_logs | jlf
```

It will output the logs in a more colorful and readable format:
<img width="700" alt="Screenshot 2024-02-09 at 12 31 37 AM" src="https://github.com/PoOnesNerfect/jlf/assets/32286177/32b347cd-f56a-4db3-b0e9-99e9d24624e6">

## Installation

### Cargo

```sh
cargo install jlf
```

### Manual

You can also clone the repo and install it manually.

```sh
git clone https://github.com/PoOnesNerfect/jlf.git
cd jlf
cargo install --path . --locked
```

## Custom Formatting

You can optionally provide your custom format.

```sh
cat ./examples/dummy_logs | jlf '{timestamp:dimmed} {level|lvl:blue} {message|msg|body}'
```

Above will print the logs with dimmed timestamp, blue level and message as is.
Above format is actually the default format.

`{timestamp:dimmed}` means that the cli will look for `timestamp` in the json and color it with `dimmed` color.

`level|lvl` means that the cli will look for `level` and `lvl` in the json and use the first one it finds.

### Additional Formatting

You can also only customize the additional formatting with `-a` flag.

```sh
cat ./examples/dummy_logs | jlf -a '\n{data}'
```

Above will print the logs with additional data in a new line.
This is also the default behavior.

## Styling

You can provide styles to the values by providing styles after the `:`.

```sh
cat ./examples/dummy_logs | jlf '{timestamp:dimmed} {level|lvl:bright blue,bg=red,bold} {message|msg|body:fg=bright white}'
```

If you have multiple styles, you can separate them with `,`.

You can optionally provide the style type before the `=`. If you don't provide it, it will default to `fg`.

### Available Colors

You can view all available colors in [colors.md](https://github.com/PoOnesNerfect/jlf/blob/main/colors.md).
