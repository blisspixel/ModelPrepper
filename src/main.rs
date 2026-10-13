use clap::Parser;
use modelprepper::cli::{Cli, CommandOutput, execute, pretty_output};
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // The Rust runtime ignores SIGPIPE. A closed stdout is a clean exit, not a panic.
    let cli = Cli::parse();
    let pretty = pretty_output(cli.pretty, io::stdout().is_terminal());
    match execute(cli) {
        Ok(CommandOutput::Json(output)) => {
            let ok = output
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            finish_json(&output, pretty, ok)
        }
        Ok(CommandOutput::Text(text)) => finish_text(&text),
        Err(error) => {
            let value = serde_json::json!({"schema_version": 1, "error": error});
            match write_json(io::stderr().lock(), &value, pretty) {
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                _ => ExitCode::FAILURE,
            }
        }
    }
}

fn finish_json(value: &serde_json::Value, pretty: bool, ok: bool) -> ExitCode {
    match write_json(io::stdout().lock(), value, pretty) {
        Ok(()) => {
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            let value = serde_json::json!({
                "schema_version": 1,
                "error": {"code": "output_failed", "message": error.to_string()}
            });
            let _ = write_json(io::stderr().lock(), &value, false);
            ExitCode::FAILURE
        }
    }
}

fn finish_text(text: &str) -> ExitCode {
    match writeln!(io::stdout().lock(), "{text}") {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn write_json(mut out: impl Write, value: &serde_json::Value, pretty: bool) -> io::Result<()> {
    let result = if pretty {
        serde_json::to_writer_pretty(&mut out, value)
    } else {
        serde_json::to_writer(&mut out, value)
    };
    if let Err(error) = result {
        return Err(match error.io_error_kind() {
            Some(kind) => io::Error::new(kind, error),
            None => io::Error::other(error),
        });
    }
    writeln!(out)
}
