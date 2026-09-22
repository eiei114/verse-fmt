mod cli;
mod config;
mod files;
mod format;
mod indent;
mod lex;
mod report;
mod source;
mod syntax;
mod write;

use std::io::{Read, Write};
use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    let color = report::color(cli.color, true);
    let result = cli.validate().and_then(|()| run(cli));
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if color {
                eprintln!("\x1b[31mverse-fmt: {error}\x1b[0m");
            } else {
                eprintln!("verse-fmt: {error}");
            }
            ExitCode::from(2)
        }
    }
}

fn run(cli: cli::Cli) -> Result<u8, String> {
    let config = config::resolve(cli.config.as_deref(), cli.stdin_filepath.as_deref())?;
    if cli.show_config {
        std::io::stdout()
            .lock()
            .write_all(config.show()?.as_bytes())
            .map_err(|e| e.to_string())?;
        return Ok(0);
    }
    let stdin = cli.paths[0].as_os_str() == "-";
    let mut rendered = Vec::new();
    if stdin {
        let label = cli
            .stdin_filepath
            .as_deref()
            .map(|p| {
                p.to_str()
                    .map(|s| s.replace('\\', "/"))
                    .ok_or("stdin path must be Unicode")
            })
            .transpose()?
            .unwrap_or_else(|| "<stdin>".into());
        let bytes = read(std::io::stdin())?;
        rendered.push(render(
            label,
            write::Snapshot::stdin(bytes),
            &config.settings.format,
        )?);
    } else {
        let discovered = files::discover(&cli.paths, &config.root, &config.settings.files.exclude)?;
        if cli.verbose {
            for excluded in &discovered.excluded {
                eprintln!("{excluded}");
            }
            eprintln!("{} eligible .verse file(s)", discovered.paths.len());
        }
        if !discovered.errors.is_empty() {
            return Err(discovered.errors.join("\n"));
        }
        let mut total = 0;
        let mut errors = Vec::new();
        for path in discovered.paths {
            let label = files::label(&path, &config.root)?;
            let result = (|| {
                let snapshot = write::Snapshot::read(&path).map_err(|e| format!("{label}: {e}"))?;
                total += snapshot.bytes.len();
                if total > files::MAX_TOTAL_BYTES {
                    return Err("inputs exceed the 64 MiB total limit".into());
                }
                render(label, snapshot, &config.settings.format)
            })();
            match result {
                Ok(item) => rendered.push(item),
                Err(e) => errors.push(e),
            }
            if total > files::MAX_TOTAL_BYTES {
                break;
            }
        }
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
    }
    if cli.write {
        let checked = rendered.len();
        let mut pending = Vec::new();
        for (label, snapshot, output) in rendered {
            if snapshot.bytes != output.as_bytes() {
                // Stage and lock every changed file before committing any file.
                pending.push((label, write::prepare(snapshot, output.as_bytes())?));
            }
        }
        let planned: Vec<_> = pending.iter().map(|(label, _)| label.clone()).collect();
        for (index, (label, replacement)) in pending.into_iter().enumerate() {
            match replacement.commit() {
                Ok(Some(warning)) => eprintln!("{label}: {warning}"),
                Ok(None) => (),
                Err(e) => {
                    return Err(format!(
                        "{label}: {e}\ncompleted: {:?}\nnot attempted: {:?}",
                        &planned[..index],
                        &planned[index + 1..]
                    ));
                }
            }
        }
        eprintln!("checked {checked} file(s), changed {}", planned.len());
        return Ok(0);
    }
    let mut changed = false;
    let mut stdout = std::io::stdout().lock();
    for (label, snapshot, output) in rendered {
        let source = std::str::from_utf8(&snapshot.bytes).map_err(|e| e.to_string())?;
        let different = source != output;
        changed |= different;
        if cli.check {
            if different {
                writeln!(stdout, "{label}").map_err(|e| e.to_string())?;
            }
        } else if cli.diff {
            if different {
                stdout
                    .write_all(
                        report::diff(&label, source, &output, report::color(cli.color, false))
                            .as_bytes(),
                    )
                    .map_err(|e| e.to_string())?;
            }
        } else {
            stdout
                .write_all(output.as_bytes())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(u8::from(changed && (cli.check || cli.diff)))
}

fn read(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((source::MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > source::MAX_SOURCE_BYTES {
        return Err("source exceeds the 8 MiB limit".into());
    }
    Ok(bytes)
}

fn render(
    label: String,
    snapshot: write::Snapshot,
    options: &format::Options,
) -> Result<(String, write::Snapshot, String), String> {
    let source =
        source::Source::from_bytes(&snapshot.bytes).map_err(|e| format!("{label}: {e}"))?;
    let output = format::format(&source, options).map_err(|e| {
        let (line, column) = source.position(e.offset);
        format!("{label}:{line}:{column}: {}", e.message)
    })?;
    Ok((label, snapshot, output))
}
