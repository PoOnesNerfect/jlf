use std::io;
use std::io::Error;

fn main() {
    if let Err(e) = jlf::run() {
        if let Some(ioe) = e.root_cause().downcast_ref::<Error>() {
            if ioe.kind() == io::ErrorKind::BrokenPipe {
                // Exit cleanly if the pipe reader disconnected
                std::process::exit(0);
            }
        }
        eprintln!("{e:?}");
    }
}
