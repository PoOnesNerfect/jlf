fn main() {
    if let Err(e) = jlf::run() {
        eprintln!("{e:?}");
    }
}
