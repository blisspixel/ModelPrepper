use clap::Parser;
use modelprepper::cli::{
    Cli, ColorMode, CommandOutput, View, app_error_text, clap_error_text, execute_in,
    terminal_columns,
};
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // The Rust runtime ignores SIGPIPE. A closed stdout is a clean exit, not a panic.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return clap_exit(error),
    };
    let stdout_tty = io::stdout().is_terminal();
    let stderr_tty = io::stderr().is_terminal();
    let view = View {
        text: !cli.json && (stdout_tty || std::env::var_os("MODELPREPPER_TEXT").is_some()),
        json: cli.json,
        color: color_enabled(cli.color, stdout_tty),
        stderr_terminal: stderr_tty,
        columns: terminal_columns(),
    };
    let pretty = cli.pretty || stdout_tty;
    let force_json = cli.json;
    let error_color = color_enabled(cli.color, stderr_tty);
    match execute_in(cli, view) {
        Ok(CommandOutput::Json(output)) => {
            let ok = output
                .get("ok")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            finish_json(&output, pretty, ok)
        }
        Ok(CommandOutput::Text { body, ok }) => finish_text(&body, ok),
        Err(error) => {
            if force_json || !stderr_tty {
                let value = serde_json::json!({"schema_version": 1, "error": error});
                match write_json(io::stderr().lock(), &value, pretty) {
                    Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                    _ => ExitCode::FAILURE,
                }
            } else {
                let text = app_error_text(&error, error_color, terminal_columns());
                match writeln!(io::stderr().lock(), "{text}") {
                    Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                    _ => ExitCode::FAILURE,
                }
            }
        }
    }
}

fn clap_exit(error: clap::Error) -> ExitCode {
    let code = ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2));
    if matches!(
        error.kind(),
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
    ) {
        if error.kind() == clap::error::ErrorKind::DisplayHelp {
            let text = group_root_help(&error.to_string());
            let _ = write!(io::stdout(), "{text}");
            if !text.ends_with('\n') {
                let _ = writeln!(io::stdout());
            }
        } else {
            let _ = error.print();
        }
        return code;
    }
    let stderr_tty = io::stderr().is_terminal();
    if cli_json_from_env() || !stderr_tty {
        let value = serde_json::json!({
            "schema_version": 1,
            "error": {"code": "invalid_usage", "message": error.to_string()}
        });
        let _ = write_json(io::stderr().lock(), &value, false);
        return code;
    }
    let text = clap_error_text(
        &error.to_string(),
        color_enabled(ColorMode::Auto, true),
        terminal_columns(),
    );
    let _ = writeln!(io::stderr().lock(), "{text}");
    code
}

/// Clap 4 applies `next_help_heading` to arguments, not to subcommands.
/// Root help still lists one command block, so the workflow groups are inserted here.
fn group_root_help(text: &str) -> String {
    if !text
        .lines()
        .any(|line| line.trim_start().starts_with("config "))
    {
        return text.to_owned();
    }
    let mut out = String::new();
    for line in text.lines() {
        if line == "Commands:" {
            out.push_str("Setup:\n");
            continue;
        }
        let name = line.split_whitespace().next().unwrap_or("");
        if name == "resolve" {
            out.push_str("\nReview a source:\n");
        } else if name == "verify-local" {
            out.push_str("\nLocal copies:\n");
        } else if name == "plan" {
            out.push_str("\nPreview:\n");
        } else if name == "help" {
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn color_never_in_args() -> bool {
    let args: Vec<String> = std::env::args().collect();
    args.iter().any(|arg| arg == "--color=never")
        || args
            .windows(2)
            .any(|pair| pair[0] == "--color" && pair[1] == "never")
}

fn cli_json_from_env() -> bool {
    std::env::args().any(|arg| arg == "--json")
}

fn color_enabled(mode: ColorMode, tty: bool) -> bool {
    match mode {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            tty && std::env::var_os("NO_COLOR").is_none()
                && std::env::var("TERM").ok().as_deref() != Some("dumb")
                && !color_never_in_args()
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

fn finish_text(text: &str, ok: bool) -> ExitCode {
    match writeln!(io::stdout().lock(), "{text}") {
        Ok(()) => {
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
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
