fn main() {
    match stax_gui::startup::parse_startup_command(std::env::args_os().skip(1)) {
        Ok(stax_gui::startup::StartupCommand::Run(repository)) => stax_gui::run(repository),
        Ok(stax_gui::startup::StartupCommand::PrintVersion) => {
            println!("stax-gui {}", env!("CARGO_PKG_VERSION"));
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
