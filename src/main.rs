mod cli;

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

fn run(_cli: cli::Cli) -> Result<u8, String> {
    Err("formatter engine not implemented yet; no files were changed".into())
}
