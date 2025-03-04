use color_eyre::eyre::Result;

fn main() {
    if let Err(e) = jlf::run() {
        eprintln!("{e:?}");
    }
}
