//! Versioned machine-readable report for `--output-format json`.
//!
//! The envelope is deterministic: no timestamps, no absolute paths, no hashes
//! of machine state. Field names and ordering are the contract described in
//! `docs/report-contract.md` and `docs/diagnostics.schema.json`.
use serde::Serialize;

use crate::source::Stage;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Unchanged,
    Formatted,
    WouldFormat,
    Refused,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: &'static str,
    pub version: &'static str,
}

impl Default for Tool {
    fn default() -> Self {
        Self {
            name: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub files_checked: usize,
    pub files_changed: usize,
    pub files_unchanged: usize,
    pub files_refused: usize,
    pub complete: bool,
}

#[derive(Debug, Serialize)]
pub struct Reason {
    pub stage: Stage,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct FileReport {
    pub path: String,
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
}

#[derive(Debug, Serialize)]
pub struct ExecutionError {
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Envelope {
    pub schema_version: u32,
    pub tool: Tool,
    pub summary: Summary,
    pub files: Vec<FileReport>,
    pub execution_errors: Vec<ExecutionError>,
}

impl Envelope {
    pub fn new(files: Vec<FileReport>, execution_errors: Vec<ExecutionError>) -> Self {
        let files_unchanged = count(&files, Status::Unchanged);
        let files_refused = count(&files, Status::Refused);
        Self {
            schema_version: SCHEMA_VERSION,
            tool: Tool::default(),
            summary: Summary {
                files_checked: files.len(),
                files_changed: count(&files, Status::Formatted)
                    + count(&files, Status::WouldFormat),
                files_unchanged,
                files_refused,
                complete: execution_errors.is_empty(),
            },
            files,
            execution_errors,
        }
    }

    /// One object, compact, no trailing newline and no other bytes.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("report envelope is always serializable")
    }
}

fn count(files: &[FileReport], status: Status) -> usize {
    files.iter().filter(|file| file.status == status).count()
}

/// The single reason recorded for a source the formatter would rewrite.
pub fn change_reason() -> Reason {
    Reason {
        stage: Stage::Layout,
        message: "formatter output differs from the source".into(),
        offset: None,
    }
}
