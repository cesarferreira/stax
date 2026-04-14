fn main() {
    let result = stax::cli::run();

    if let Err(err) = result {
        // Check if it's a StaxError with a specific exit code
        if let Some(stax_err) = err.downcast_ref::<stax::errors::StaxError>() {
            eprintln!("Error: {}", stax_err);
            std::process::exit(stax_err.exit_code());
        }

        // Check if it's a ConflictStopped (exit code 2)
        if err.downcast_ref::<stax::errors::ConflictStopped>().is_some() {
            // ConflictStopped already printed the error message
            std::process::exit(stax::errors::exit_codes::CONFLICT);
        }

        // Default: print error and exit with code 1
        eprintln!("Error: {:#}", err);
        std::process::exit(stax::errors::exit_codes::GENERAL);
    }
}
