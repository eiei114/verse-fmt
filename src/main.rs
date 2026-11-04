mod cli;
mod config;
mod files;
mod format;
mod indent;
mod json;
mod lex;
mod limits;
mod machine;
mod report;
#[cfg(test)]
mod robustness;
mod source;
mod syntax;
mod write;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
///
/// Every machine copy of a diagnostic is sanitized against `root`, so a message
/// built from an absolute path — which the text path still prints verbatim —
/// never reaches `reason` or `executionErrors`.
struct Machine {
    files: Vec<FileReport>,
    errors: Vec<ExecutionError>,
    root: Option<PathBuf>,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            errors: Vec::new(),
            // Until the configuration resolves, the working directory is the
            // best available root; a configuration failure is reported against
            // it and can still not leak an absolute path.
            root: std::env::current_dir().ok(),
        }
    }
}

impl Machine {
    fn set_root(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
    }

    fn sanitize(&self, message: &str) -> String {
        machine::sanitize(message, self.root.as_deref().unwrap_or(Path::new("")))
    }

    fn error(&mut self, path: Option<String>, message: &str) {
        self.errors.push(ExecutionError {
            path,
            message: self.sanitize(message),
        });
    }

    fn file(&mut self, path: String, status: Status, reason: Option<Reason>) {
        self.files.push(FileReport {
            path,
            status,
            reason,
        });
    }

    fn refused(&mut self, failure: &FileFailure) {
        let message = self.sanitize(&failure.message);
        self.file(
            failure.label.clone(),
            Status::Refused,
            Some(Reason {
                stage: failure.stage,
                message,
                offset: Some(failure.offset),
            }),
        );
        self.error(Some(failure.label.clone()), &failure.message);
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
}

/// One successfully analyzed source, before any mode-specific output.
struct Analyzed {
    label: String,
    snapshot: write::Snapshot,
    output: String,
    different: bool,
}

fn fail<T>(machine: &mut Machine, path: Option<String>, message: String) -> Result<T, String> {
    machine.error(path, &message);
    Err(message)
}

fn run(cli: &cli::Cli, machine: &mut Machine) -> Result<u8, String> {
    let config = match config::resolve(cli.config.as_deref(), cli.stdin_filepath.as_deref()) {
        Ok(config) => config,
        Err(error) => return fail(machine, None, error),
    };
    machine.set_root(&config.root);
    if cli.show_config {
        std::io::stdout()
            .lock()
            .write_all(config.show()?.as_bytes())
            .map_err(|e| e.to_string())?;
        return Ok(0);
    }
    // A content change is `would-format` until the bytes are on disk: only the
    // write phase, after that file's own commit succeeded, promotes an entry to
    // `formatted`. Nothing else can claim a change was applied.
    let status_for = |different: bool| match different {
        true => Status::WouldFormat,
        false => Status::Unchanged,
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
                machine.error(None, error);
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
                    Err(error) => return Err(write_failure(machine, item.label, &error)),
                }
            }
        }
        let changed = pending.len();
        commit_all(machine, pending)?;
        eprintln!("checked {checked} file(s), changed {changed}");
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

/// Commit every staged replacement in order, promoting each entry to
/// `formatted` only after its own commit returned success. A batch that stops
/// midway therefore reports exactly what reached the disk; the files that were
/// staged but never committed keep `would-format`.
fn commit_all(machine: &mut Machine, pending: Vec<(String, write::Pending)>) -> Result<(), String> {
    commit_all_using(machine, pending, write::Pending::commit)
}

fn commit_all_using(
    machine: &mut Machine,
    pending: Vec<(String, write::Pending)>,
    mut commit: impl FnMut(write::Pending) -> Result<Option<String>, String>,
) -> Result<(), String> {
    let planned: Vec<String> = pending.iter().map(|(label, _)| label.clone()).collect();
    for (index, (label, replacement)) in pending.into_iter().enumerate() {
        match commit(replacement) {
            Ok(Some(warning)) => eprintln!("{label}: {warning}"),
            Ok(None) => (),
            Err(error) => {
                return Err(write_failure(
                    machine,
                    label,
                    &format!(
                        "{error}\ncompleted: {:?}\nnot attempted: {:?}",
                        &planned[..index],
                        &planned[index + 1..]
                    ),
                ));
            }
        }
        machine.restatus(&label, Status::Formatted, Some(json::change_reason()));
    }
    Ok(())
}

/// A write failure is a whole-invocation execution error for one known path,
/// never a refused file: a refusal means the source was analyzed and rejected,
/// and `docs/report-contract.md` reserves `reason.stage = write` for exactly
/// this. Statuses already recorded are left alone, so a staged but uncommitted
/// file stays `would-format` instead of claiming bytes that never landed.
fn write_failure(machine: &mut Machine, label: String, error: &str) -> String {
    let text = format!("{label}: {error}");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(root: &Path) -> Machine {
        let mut machine = Machine::default();
        machine.set_root(root);
        machine
    }

    /// A commit that fails midway must not retroactively bless the files that
    /// were staged but never replaced, and must not invent a refusal.
    #[test]
    fn a_commit_failure_reports_only_what_reached_the_disk() {
        let root = tempfile::tempdir().unwrap();
        for (name, body, _) in [
            ("a.verse", "A:=1\n", "A := 1\n"),
            ("b.verse", "B:=2\n", "B := 2\n"),
        ] {
            std::fs::write(root.path().join(name), body).unwrap();
        }
        let mut machine = machine(root.path());
        let mut pending = Vec::new();
        for (name, formatted) in [("a.verse", "A := 1\n"), ("b.verse", "B := 2\n")] {
            let snapshot = write::Snapshot::read(&root.path().join(name)).unwrap();
            pending.push((
                name.to_owned(),
                write::prepare(snapshot, formatted.as_bytes()).unwrap(),
            ));
            machine.file(
                name.to_owned(),
                Status::WouldFormat,
                Some(json::change_reason()),
            );
        }

        let mut commits = 0;
        let error = commit_all_using(&mut machine, pending, |replacement| {
            commits += 1;
            if commits == 2 {
                return Err(format!(
                    "{}: injected replacement failure",
                    root.path().join("b.verse").display()
                ));
            }
            replacement.commit()
        })
        .unwrap_err();

        assert_eq!(commits, 2, "{error}");
        assert_eq!(machine.files[0].status, Status::Formatted, "{error}");
        assert_eq!(machine.files[1].status, Status::WouldFormat, "{error}");
        assert!(
            machine
                .files
                .iter()
                .all(|file| file.status != Status::Refused),
            "{error}"
        );
        assert_eq!(machine.errors.len(), 1, "{error}");
        assert_eq!(machine.errors[0].path.as_deref(), Some("b.verse"));
        let message = &machine.errors[0].message;
        assert!(
            !message.contains(&root.path().display().to_string()),
            "{message}"
        );
        assert!(!message.contains('\\'), "{message}");
        assert_eq!(
            std::fs::read(root.path().join("a.verse")).unwrap(),
            b"A := 1\n"
        );
        assert_eq!(
            std::fs::read(root.path().join("b.verse")).unwrap(),
            b"B:=2\n"
        );
    }

    /// A staging failure happens before any commit, so every changed entry
    /// stays `would-format` and the invocation fails as a whole.
    #[test]
    fn a_staging_failure_leaves_every_changed_entry_unwritten() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.verse"), b"A:=1\n").unwrap();
        let mut machine = machine(root.path());
        machine.file(
            "a.verse".to_owned(),
            Status::WouldFormat,
            Some(json::change_reason()),
        );
        let label = "a.verse".to_owned();
        let text = write_failure(
            &mut machine,
            label,
            &format!("{}: read-only file", root.path().join("a.verse").display()),
        );
        assert!(text.contains("read-only file"), "{text}");
        assert_eq!(machine.files[0].status, Status::WouldFormat);
        assert_eq!(
            machine.files[0].reason.as_ref().unwrap().stage,
            Stage::Layout
        );
        assert_eq!(machine.errors.len(), 1);
        assert!(!machine.errors[0].message.contains('\\'));
        assert_eq!(machine.errors[0].message, "a.verse: read-only file");
    }
}
