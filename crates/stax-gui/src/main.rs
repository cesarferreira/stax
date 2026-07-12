use std::path::PathBuf;

fn main() {
    let repository = std::env::args_os().nth(1).map(PathBuf::from);
    stax_gui::run(repository);
}
