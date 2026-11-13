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
use source::{Failure, Stage, Trace};

fn main() -> ExitCode {
    let cli = cli::Cli::parse();
    let color = report::color(cli.color, true);
    if let Err(error) = cli.validate() {
        diagnostic(color, &error);
        return ExitCode::from(2);
    }
    let mut machine = Machine::default();
    let result = run(&cli, &mut machine);
    let explanations = std::mem::take(&mut machine.explain);
    if cli.output_format == cli::OutputFormat::Json {
        // Exactly one object on stdout, and nothing else: a failure still
        // reports the analyzed state instead of dropping the envelope.
        let envelope = json::Envelope::new(machine.files, machine.errors);
        if let Err(error) = std::io::stdout()
            .lock()
            .write_all(envelope.encode().as_bytes())
        {
            diagnostic(
                color,
                &format!("{error} [stage={}]", Stage::Reporting.name()),
            );
            return ExitCode::from(2);
        }
    }
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            diagnostic(color, &error);
            if cli.explain {
                for explanation in explanations {
                    eprintln!("{explanation}");
                }
            }
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
    /// `--explain` blocks, one per refused source, in analysis order. Kept apart
    /// from `errors` because they are a text-only diagnostic aid and never part
    /// of the machine envelope.
    explain: Vec<String>,
    root: Option<PathBuf>,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            errors: Vec::new(),
            explain: Vec::new(),
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

    /// Report form of a text label: unchanged when it is already relative.
    fn path(&self, label: &str) -> String {
        machine::path(label, self.root.as_deref().unwrap_or(Path::new("")))
    }

    fn error(&mut self, path: Option<String>, message: &str) {
        self.errors.push(ExecutionError {
            path: path.map(|path| self.path(&path)),
            message: self.sanitize(message),
        });
    }

    fn file(&mut self, path: String, status: Status, reason: Option<Reason>) {
        self.files.push(FileReport {
            path: self.path(&path),
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
                class: failure.class,
                underlying: failure.underlying.clone(),
            }),
        );
        self.error(Some(failure.label.clone()), &failure.message);
        self.explain.push(self.sanitize(&failure.explanation()));
    }

    /// Replace the status already recorded for a path. Paths are unique per
    /// invocation, so no second report entry is ever appended.
    fn restatus(&mut self, path: &str, status: Status, reason: Option<Reason>) {
        let path = self.path(path);
        if let Some(entry) = self.files.iter_mut().find(|entry| entry.path == path) {
            entry.status = status;
            entry.reason = reason;
        }
    }
}

/// A refusal for one analyzed source. `text` is the exact stderr fragment the
/// text path prints today, plus this slice's stable `[stage=…]` marker; `message`
/// is the same diagnostic without the path or marker.
struct FileFailure {
    label: String,
    stage: Stage,
    offset: usize,
    /// Set only where one stage has two distinguishable origins; `guard` and
    /// `parse` share `parser` and must not be conflated.
    class: Option<source::Class>,
    /// Failures recorded after the primary one, in discovery order.
    underlying: Vec<source::Underlying>,
    /// Stage order the source actually entered, ending at the failing stage.
    stages: Vec<Stage>,
    message: String,
    text: String,
}

impl FileFailure {
    /// Failure that carries no source position (decode, read, limit or write).
    fn message(label: &str, stage: Stage, offset: usize, message: String, trace: &Trace) -> Self {
        let stages = stages_with(trace, stage);
        Self {
            label: label.to_owned(),
            stage,
            offset,
            class: None,
            underlying: Vec::new(),
            stages,
            text: format!("{label}: {message} [stage={}]", stage.name()),
            message,
        }
    }

    /// Failure reported at a line/column, matching the positioned stderr form.
    fn positioned(label: &str, source: &source::Source, failure: Failure, trace: &Trace) -> Self {
        let (line, column) = source.position(failure.offset);
        let message = format!("{line}:{column}: {}", failure.message);
        let class = failure.class;
        let mut text = format!("{label}:{message} [stage={}]", failure.stage.name());
        if let Some(class) = class {
            text.push_str(&format!(" [class={}]", class.name()));
        }
        // A guard refusal stays primary; an underlying parse error is reported
        // next to it, at its own position, with its node kind, so a reader does
        // not fix a guard symptom and discover a parse error afterwards.
        for underlying in &failure.underlying {
            let (line, column) = source.position(underlying.offset);
            text.push_str(&format!(
                "\n{label}:{line}:{column}: {} [stage={}] [class={}]",
                underlying.message,
                underlying.stage.name(),
                underlying.class.name()
            ));
        }
        Self {
            label: label.to_owned(),
            stage: failure.stage,
            offset: failure.offset,
            class,
            underlying: failure.underlying,
            stages: stages_with(trace, failure.stage),
            // Byte-identical to the pre-slice text path apart from the marker:
            // no space after the label.
            text,
            message,
        }
    }

    /// The complete failure chain for `--explain`: the stage order that actually
    /// ran, then every recorded failure with its primary/secondary class.
    fn explanation(&self) -> String {
        let mut block = format!(
            "verse-fmt explain: {}\n  stages: {}",
            self.label,
            self.stages
                .iter()
                .map(|stage| stage.name())
                .collect::<Vec<_>>()
                .join(" -> ")
        );
        block.push_str(&format!(
            "\n  failure 1 (primary, {}/{}, offset {}): {}",
            self.stage.name(),
            self.class.map_or("none", |class| class.name()),
            self.offset,
            self.message
        ));
        for (index, underlying) in self.underlying.iter().enumerate() {
            block.push_str(&format!(
                "\n  failure {} (secondary, {}/{}, {}, offset {}): {}",
                index + 2,
                underlying.stage.name(),
                underlying.class.name(),
                underlying.node_kind,
                underlying.offset,
                underlying.message
            ));
        }
        block
    }
}

/// The executed stages, ending at the stage that stopped processing. A failure
/// that happened before any traced stage (a discovery or read failure) reports
/// just its own stage.
///
/// The trace is not monotonic: `validation` re-enters `lexer` and `parser` to
/// reparse the formatter's own output, so a structure, layout or validation
/// failure can happen while the trace tail is already `parser`. Only appending a
/// stage the trace has not reached at all would leave such a trace ending at the
/// reparse's `parser` instead of the stage that stopped processing, so the
/// comparison is against the current tail.
fn stages_with(trace: &Trace, stage: Stage) -> Vec<Stage> {
    let mut stages = trace.stages().to_vec();
    if stages.last() != Some(&stage) {
        stages.push(stage);
    }
    stages
}

/// One successfully analyzed source, before any mode-specific output.
struct Analyzed {
    label: String,
    snapshot: write::Snapshot,
    output: String,
    different: bool,
}

fn fail<T>(
    machine: &mut Machine,
    stage: Stage,
    path: Option<String>,
    message: String,
) -> Result<T, String> {
    machine.error(path, &message);
    // The text diagnostic names the stage that stopped processing; the machine
    // copy of the same message stays marker-free so `executionErrors[].message`
    // keeps its documented shape.
    Err(format!("{message} [stage={}]", stage.name()))
}

fn run(cli: &cli::Cli, machine: &mut Machine) -> Result<u8, String> {
    let config = match config::resolve(cli.config.as_deref(), cli.stdin_filepath.as_deref()) {
        Ok(config) => config,
        Err(error) => return fail(machine, Stage::Discovery, None, error),
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
            Err(error) => return fail(machine, Stage::Discovery, None, error.to_owned()),
        };
        let bytes = match read(std::io::stdin()) {
            Ok(bytes) => bytes,
            // A read/limit failure before any byte is analyzed has no file to
            // refuse, and the human diagnostic must stay byte-identical.
            Err(error) => return fail(machine, Stage::Source, None, error),
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
                Err(error) => return fail(machine, Stage::Discovery, None, error),
            };
        if cli.verbose {
            for excluded in &discovered.excluded {
                eprintln!("{excluded}");
            }
            eprintln!("{} eligible .verse file(s)", discovered.paths.len());
        }
        let mut discovery_failures: Vec<String> = Vec::new();
        if !discovered.errors.is_empty() {
            for error in &discovered.errors {
                machine.error(None, error);
            }
            let text = discovered
                .errors
                .iter()
                .map(|error| format!("{error} [stage={}]", Stage::Discovery.name()))
                .collect::<Vec<_>>()
                .join("\n");
            if !discovered.limit_exceeded {
                return Err(text);
            }
            // A documented limit stops the walk, not the analysis: every source
            // found before it still reports its own result, which is the
            // partial-completion contract `docs/resource-limits.md` describes.
            // The limit itself remains an execution error, and any refusal
            // below still prevents the write phase, so a limited run writes
            // nothing.
            discovery_failures.push(text);
        }
        let mut total = 0;
        let mut failures: Vec<Box<FileFailure>> = Vec::new();
        for path in discovered.paths {
            let label = match files::label(&path, &config.root) {
                Ok(label) => label,
                Err(error) => return fail(machine, Stage::Discovery, None, error),
            };
            let snapshot = match write::Snapshot::read(&path) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    let failure = Box::new(FileFailure::message(
                        &label,
                        Stage::Source,
                        0,
                        error,
                        &Trace::default(),
                    ));
                    machine.refused(&failure);
                    failures.push(failure);
                    continue;
                }
            };
            total += snapshot.bytes.len();
            if total > files::MAX_TOTAL_BYTES {
                let failure = Box::new(FileFailure::message(
                    &label,
                    Stage::Source,
                    0,
                    "inputs exceed the 64 MiB total limit".into(),
                    &Trace::default(),
                ));
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
        if !failures.is_empty() || !discovery_failures.is_empty() {
            let mut lines = discovery_failures;
            lines.extend(failures.iter().map(|failure| failure.text.clone()));
            return Err(lines.join("\n"));
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
/// and `docs/report-contract.md` documents `write` as an `executionErrors` stage
/// for exactly this. Statuses already recorded are left alone, so a staged but
/// uncommitted file stays `would-format` instead of claiming bytes that never
/// landed.
fn write_failure(machine: &mut Machine, label: String, error: &str) -> String {
    // The stage marker is text-only: the machine copy stays the diagnostic
    // without the path prefix, as `executionErrors[].message` documents. It goes
    // on the first line so a multi-line batch report stays greppable.
    let (first, rest) = match error.split_once('\n') {
        Some((first, rest)) => (first, format!("\n{rest}")),
        None => (error, String::new()),
    };
    let text = format!("{label}: {first} [stage={}]{}", Stage::Write.name(), rest);
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
) -> Result<Analyzed, Box<FileFailure>> {
    let mut trace = Trace::default();
    trace.enter(Stage::Source);
    let source = source::Source::from_bytes(&snapshot.bytes).map_err(|failure| {
        Box::new(FileFailure::message(
            label,
            failure.stage,
            failure.offset,
            failure.to_string(),
            &trace,
        ))
    })?;
    let output = format::format_traced(&source, options, &mut trace)
        .map_err(|failure| Box::new(FileFailure::positioned(label, &source, failure, &trace)))?;
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

    /// The stage list `--explain` prints must end at the stage that stopped
    /// processing even when `validation` re-entered `lexer`/`parser` to reparse
    /// the formatter's own output. Appending only a stage the trace never
    /// reached would print `... -> parser` for a structure, layout or validation
    /// failure that happened after the reparse.
    #[test]
    fn stages_end_at_the_stage_that_stopped_processing_after_a_validation_reparse() {
        let mut trace = Trace::default();
        for stage in [
            Stage::Source,
            Stage::Lexer,
            Stage::Parser,
            Stage::Structure,
            Stage::Layout,
            Stage::Validation,
            Stage::Lexer,
            Stage::Parser,
        ] {
            trace.enter(stage);
        }
        let reentered = trace.stages().to_vec();

        // Stage failures of the second `indent::analyze`, `layout`, and the
        // preservation/idempotence checks are all already contained in the
        // trace: each still has to end the printed list.
        for stage in [Stage::Structure, Stage::Layout, Stage::Validation] {
            let stages = stages_with(&trace, stage);
            assert_eq!(&stages[..reentered.len()], &reentered[..], "{stage:?}");
            assert_eq!(stages.last(), Some(&stage), "{stage:?}");
            assert_eq!(stages.len(), reentered.len() + 1, "{stage:?}");
        }

        // A failure in the stage the trace already ends at must not repeat it.
        assert_eq!(stages_with(&trace, Stage::Parser), reentered);
        assert_eq!(
            stages_with(&trace, Stage::Lexer).last(),
            Some(&Stage::Lexer)
        );

        // A failure before any traced stage reports just its own stage.
        let empty = Trace::default();
        assert_eq!(
            stages_with(&empty, Stage::Discovery),
            vec![Stage::Discovery]
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
