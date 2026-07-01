fn main() {
    if let Err(error) = agent_token_usage::cli::run_from_env() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
