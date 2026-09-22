mod cli;
mod format;
mod lex;
mod source;
mod syntax;

use std::io::{Read, Write};
use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    let result = cli.validate().and_then(|()| run(cli));
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("verse-fmt: {error}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: cli::Cli) -> Result<u8, String> {
    if cli.write || cli.diff || cli.config.is_some() || cli.show_config {
        return Err("project configuration, diff, and safe write are not implemented yet; no files were changed".into());
    }
    if cli.paths.len() != 1 || cli.paths[0].is_dir() {
        return Err(
            "project traversal is not implemented yet; provide one file or stdin '-'".into(),
        );
    }
    let path = &cli.paths[0];
    let stdin = path.as_os_str() == "-";
    let label = if stdin {
        cli.stdin_filepath
            .as_ref()
            .map_or_else(|| "<stdin>".into(), |p| p.to_string_lossy().into_owned())
    } else {
        path.to_string_lossy().into_owned()
    };
    let mut bytes = Vec::new();
    let reader: Box<dyn Read> = if stdin {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(path).map_err(|e| format!("{label}: {e}"))?)
    };
    reader
        .take((source::MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{label}: {e}"))?;
    let source = source::Source::from_bytes(&bytes).map_err(|e| format!("{label}: {e}"))?;
    let output = format::format(&source).map_err(|e| {
        let (line, column) = source.position(e.offset);
        format!("{label}:{line}:{column}: {}", e.message)
    })?;
    let changed = output != source.text();
    let mut stdout = std::io::stdout().lock();
    if cli.check {
        if changed {
            writeln!(stdout, "{label}").map_err(|e| e.to_string())?;
        }
        Ok(u8::from(changed))
    } else {
        stdout
            .write_all(output.as_bytes())
            .map_err(|e| e.to_string())?;
        Ok(0)
    }
}
