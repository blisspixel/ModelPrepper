use clap::Parser;
use modelprepper::cli::{Cli, execute};
use std::process::ExitCode;

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::json!({"schema_version": 1, "error": error})
            );
            ExitCode::FAILURE
        }
    }
}
