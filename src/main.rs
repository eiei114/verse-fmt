mod cli;
mod config;
mod files;
mod format;
mod indent;
mod json;
mod lex;
mod limits;
mod report;
#[cfg(test)]
mod robustness;
mod source;
mod syntax;
mod write;

use std::io::{Read, Write};
use std::process::ExitCode;

use clap::Parser;

use json::{ExecutionError, FileReport, Reason, Status};
use source::{Failure, Stage};

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    let color = report::color(cli.color, true);
    if let Err(error) = cli.validate() {
        diagnostic(color, &error);
        return ExitCode::from(2);
    }
    let mut machine = Machine::default();
    let result = run(&cli, &mut machine);
    if cli.output_format == cli::OutputFormat::Json {
        // Exactly one object on stdout, and nothing else: a failure still
        // reports the analyzed state instead of dropping the envelope.
        let envelope = json::Envelope::new(machine.files, machine.errors);
        if let Err(error) = std::io::stdout()
            .lock()
            .write_all(envelope.encode().as_bytes())
        {
            diagnostic(color, &error.to_string());
            return ExitCode::from(2);
        }
    }
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            diagnostic(color, &error);
            ExitCode::from(2)
        }
    }
}

fn diagnostic(color: bool, error: &str) {
    if color {
        eprintln!("\x1b[31mverse-fmt: {error}\x1b[0m");
    } else {
        eprintln!("verse-fmt: {error}");
    }
}

/// Machine-report accumulator. `files` keeps discovery order (already sorted by
/// relative label); `errors` mirrors the stderr diagnostics one for one.
#[derive(Default)]
struct Machine {
    files: Vec<FileReport>,
    errors: Vec<ExecutionError>,
}

impl Machine {
    fn error(&mut self, path: Option<String>, message: String) {
        self.errors.push(ExecutionError { path, message });
    }

    fn file(&mut self, path: String, status: Status, reason: Option<Reason>) {
        self.files.push(FileReport {
            path,
            status,
            reason,
        });
    }

    fn refused(&mut self, failure: &FileFailure) {
        self.file(
            failure.label.clone(),
            Status::Refused,
            Some(failure.reason()),
        );
        self.error(Some(failure.label.clone()), failure.message.clone());
    }

    /// Replace the status already recorded for a path. Paths are unique per
    /// invocation, so no second report entry is ever appended.
    fn restatus(&mut self, path: &str, status: Status, reason: Option<Reason>) {
        if let Some(entry) = self.files.iter_mut().find(|entry| entry.path == path) {
            entry.status = status;
            entry.reason = reason;
        }
    }
}

/// A refusal for one analyzed source. `text` is the exact stderr fragment the
/// text path prints today; `message` is the same diagnostic without the path.
struct FileFailure {
    label: String,
    stage: Stage,
    offset: usize,
    message: String,
    text: String,
}

impl FileFailure {
    /// Failure that carries no source position (decode, read, limit or write).
    fn message(label: &str, stage: Stage, offset: usize, message: String) -> Self {
        Self {
            label: label.to_owned(),
            stage,
            offset,
            text: format!("{label}: {message}"),
            message,
        }
    }

    /// Failure reported at a line/column, matching the positioned stderr form.
    fn positioned(label: &str, source: &source::Source, failure: Failure) -> Self {
        let (line, column) = source.position(failure.offset);
        let message = format!("{line}:{column}: {}", failure.message);
        Self {
            label: label.to_owned(),
            stage: failure.stage,
            offset: failure.offset,
            // Byte-identical to the pre-slice text path: no space after the label.
            text: format!("{label}:{message}"),
            message,
        }
    }

    fn reason(&self) -> Reason {
        Reason {
            stage: self.stage,
            message: self.message.clone(),
            offset: Some(self.offset),
        }
    }
}

/// One successfully analyzed source, before any mode-specific output.
struct Analyzed {
    label: String,
    snapshot: write::Snapshot,
    output: String,
    different: bool,
}

fn fail<T>(machine: &mut Machine, path: Option<String>, message: String) -> Result<T, String> {
    machine.error(path, message.clone());
    Err(message)
}

fn run(cli: &cli::Cli, machine: &mut Machine) -> Result<u8, String> {
    let config = match config::resolve(cli.config.as_deref(), cli.stdin_filepath.as_deref()) {
        Ok(config) => config,
        Err(error) => return fail(machine, None, error),
    };
    if cli.show_config {
        std::io::stdout()
            .lock()
            .write_all(config.show()?.as_bytes())
            .map_err(|e| e.to_string())?;
        return Ok(0);
    }
    // `formatted` means the file was written; every other mode only reports
    // what it would do.
    let status_for = |different: bool| match (different, cli.write) {
        (false, _) => Status::Unchanged,
        (true, true) => Status::Formatted,
        (true, false) => Status::WouldFormat,
    };
    let mut analyzed = Vec::new();
    let stdin = cli.paths[0].as_os_str() == "-";
    if stdin {
        let label = match cli
            .stdin_filepath
            .as_deref()
            .map(|p| {
                p.to_str()
                    .map(|s| s.replace('\\', "/"))
                    .ok_or("stdin path must be Unicode")
            })
            .transpose()
        {
            Ok(label) => label.unwrap_or_else(|| "<stdin>".into()),
            Err(error) => return fail(machine, None, error.to_owned()),
        };
        let bytes = match read(std::io::stdin()) {
            Ok(bytes) => bytes,
            // A read/limit failure before any byte is analyzed has no file to
            // refuse, and the human diagnostic must stay byte-identical.
            Err(error) => return fail(machine, None, error),
        };
        match render(
            &label,
            write::Snapshot::stdin(bytes),
            &config.settings.format,
        ) {
            Ok(item) => {
                let different = item.snapshot.bytes != item.output.as_bytes();
                machine.file(
                    item.label.clone(),
                    status_for(different),
                    different.then(json::change_reason),
                );
                analyzed.push(item);
            }
            Err(failure) => {
                machine.refused(&failure);
                return Err(failure.text);
            }
        }
    } else {
        let discovered =
            match files::discover(&cli.paths, &config.root, &config.settings.files.exclude) {
                Ok(discovered) => discovered,
                Err(error) => return fail(machine, None, error),
            };
        if cli.verbose {
            for excluded in &discovered.excluded {
                eprintln!("{excluded}");
            }
            eprintln!("{} eligible .verse file(s)", discovered.paths.len());
        }
        if !discovered.errors.is_empty() {
            for error in &discovered.errors {
                machine.error(None, error.clone());
            }
            return Err(discovered.errors.join("\n"));
        }
        let mut total = 0;
        let mut failures: Vec<FileFailure> = Vec::new();
        for path in discovered.paths {
            let label = match files::label(&path, &config.root) {
                Ok(label) => label,
                Err(error) => return fail(machine, None, error),
            };
            let snapshot = match write::Snapshot::read(&path) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    let failure = FileFailure::message(&label, Stage::Source, 0, error);
                    machine.refused(&failure);
                    failures.push(failure);
                    continue;
                }
            };
            total += snapshot.bytes.len();
            if total > files::MAX_TOTAL_BYTES {
                let failure = FileFailure::message(
                    &label,
                    Stage::Source,
                    0,
                    "inputs exceed the 64 MiB total limit".into(),
                );
                machine.refused(&failure);
                failures.push(failure);
                break;
            }
            match render(&label, snapshot, &config.settings.format) {
                Ok(item) => {
                    let different = item.snapshot.bytes != item.output.as_bytes();
                    machine.file(
                        item.label.clone(),
                        status_for(different),
                        different.then(json::change_reason),
                    );
                    analyzed.push(item);
                }
                Err(failure) => {
                    machine.refused(&failure);
                    failures.push(failure);
                }
            }
        }
        if !failures.is_empty() {
            return Err(failures
                .iter()
                .map(|failure| failure.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"));
        }
    }
    if cli.write {
        let checked = analyzed.len();
        let mut pending = Vec::new();
        for item in analyzed {
            if item.different {
                // Stage and lock every changed file before committing any file.
                match write::prepare(item.snapshot, item.output.as_bytes()) {
                    Ok(prepared) => pending.push((item.label, prepared)),
                    Err(error) => return Err(write_failure(machine, item.label, error)),
                }
            }
        }
        let planned: Vec<_> = pending.iter().map(|(label, _)| label.clone()).collect();
        for (index, (label, replacement)) in pending.into_iter().enumerate() {
            match replacement.commit() {
                Ok(Some(warning)) => eprintln!("{label}: {warning}"),
                Ok(None) => (),
                Err(error) => {
                    return Err(write_failure(
                        machine,
                        label,
                        format!(
                            "{error}\ncompleted: {:?}\nnot attempted: {:?}",
                            &planned[..index],
                            &planned[index + 1..]
                        ),
                    ));
                }
            }
        }
        eprintln!("checked {checked} file(s), changed {}", planned.len());
        return Ok(0);
    }
    let mut changed = false;
    let machine_mode = cli.output_format == cli::OutputFormat::Json;
    let mut stdout = (!machine_mode).then(|| std::io::stdout().lock());
    for item in analyzed {
        changed |= item.different;
        let Some(stdout) = stdout.as_mut() else {
            continue;
        };
        if cli.check {
            if item.different {
                writeln!(stdout, "{}", item.label).map_err(|e| e.to_string())?;
            }
        } else if cli.diff {
            if item.different {
                let source =
                    std::str::from_utf8(&item.snapshot.bytes).map_err(|e| e.to_string())?;
                stdout
                    .write_all(
                        report::diff(
                            &item.label,
                            source,
                            &item.output,
                            report::color(cli.color, false),
                        )
                        .as_bytes(),
                    )
                    .map_err(|e| e.to_string())?;
            }
        } else {
            stdout
                .write_all(item.output.as_bytes())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(u8::from(changed && (cli.check || cli.diff)))
}

/// A write failure is an execution error for one known path, never a silently
/// successful file.
fn write_failure(machine: &mut Machine, label: String, error: String) -> String {
    let text = format!("{label}: {error}");
    machine.restatus(
        &label,
        Status::Refused,
        Some(Reason {
            stage: Stage::Write,
            message: error.clone(),
            offset: None,
        }),
    );
    machine.error(Some(label), error);
    text
}

fn read(reader: impl Read) -> Result<Vec<u8>, String> {
    limits::read_bounded(reader, source::MAX_SOURCE_BYTES)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "source exceeds the 8 MiB limit".into())
}

fn render(
    label: &str,
    snapshot: write::Snapshot,
    options: &format::Options,
) -> Result<Analyzed, FileFailure> {
    let source = source::Source::from_bytes(&snapshot.bytes).map_err(|failure| {
        FileFailure::message(label, failure.stage, failure.offset, failure.to_string())
    })?;
    let output = format::format(&source, options)
        .map_err(|failure| FileFailure::positioned(label, &source, failure))?;
    let different = snapshot.bytes != output.as_bytes();
    Ok(Analyzed {
        label: label.to_owned(),
        snapshot,
        output,
        different,
    })
}
