//! `scripts/verify-parity.ps1` validates `parity.json`, this repository's side of
//! the paired convergence declaration, without verse-lint. Each test copies the
//! files the script reads into a scratch root, breaks exactly one of them and
//! expects the script to fail naming the field that drifted.

use std::{fs, path::Path, process::Command};

const COPIED: &[&str] = &[
    "parity.json",
    "tests/fixtures/corpus.json",
    "tests/fixtures/contract-pair.json",
    "docs/cli.md",
    "vendor/tree-sitter-verse/upstream.md",
];

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn scratch_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in COPIED {
        let target = dir.path().join(name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(repo().join(name), target).unwrap();
    }
    dir
}

fn run(root: &Path) -> (bool, String) {
    let out = Command::new("pwsh")
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(repo().join("scripts/verify-parity.ps1"))
        .arg("-Root")
        .arg(root)
        .output()
        .expect("pwsh must be available: verify.ps1 already requires PowerShell 7");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

fn edit(root: &Path, name: &str, change: impl FnOnce(String) -> String) {
    let path = root.join(name);
    let text = fs::read_to_string(&path).unwrap();
    let changed = change(text.clone());
    assert_ne!(text, changed, "the edit to {name} changed nothing");
    fs::write(path, changed).unwrap();
}

fn fails_naming(root: &Path, field: &str) {
    let (ok, text) = run(root);
    assert!(
        !ok,
        "expected a failure naming {field}, got success: {text}"
    );
    assert!(
        text.contains(&format!("parity: {field}:")),
        "expected the failure to name {field}, got: {text}"
    );
}

#[test]
fn the_committed_declaration_matches_this_repository() {
    let dir = scratch_root();
    let (ok, text) = run(dir.path());
    assert!(ok, "parity.json drifted from this repository: {text}");
}

#[test]
fn the_real_checkout_passes_without_a_scratch_copy() {
    let (ok, text) = run(repo());
    assert!(ok, "parity.json drifted from this repository: {text}");
}

#[test]
fn a_changed_corpus_fails_naming_the_hash() {
    let dir = scratch_root();
    edit(dir.path(), "tests/fixtures/corpus.json", |t| {
        t.replacen("\"cases\"", "\"cases\" ", 1)
    });
    fails_naming(dir.path(), "corpusSha256");
}

#[test]
fn a_wrong_golden_count_fails_naming_the_field() {
    let dir = scratch_root();
    edit(dir.path(), "parity.json", |t| {
        t.replace("\"goldenCaseCount\": ", "\"goldenCaseCount\": 1")
    });
    fails_naming(dir.path(), "goldenCaseCount");
}

#[test]
fn a_wrong_refusal_count_fails_naming_the_field() {
    let dir = scratch_root();
    edit(dir.path(), "parity.json", |t| {
        t.replace("\"refusalCaseCount\": ", "\"refusalCaseCount\": 1")
    });
    fails_naming(dir.path(), "refusalCaseCount");
}

#[test]
fn a_contract_version_that_disagrees_with_the_record_fails() {
    let dir = scratch_root();
    edit(dir.path(), "parity.json", |t| {
        t.replace("\"contractVersion\": 1", "\"contractVersion\": 2")
    });
    fails_naming(dir.path(), "contractVersion");
}

#[test]
fn a_contract_version_that_disagrees_with_the_cli_document_fails() {
    let dir = scratch_root();
    edit(dir.path(), "docs/cli.md", |t| {
        t.replace(
            "Shared toolchain contract version **1**.",
            "Shared toolchain contract version **2**.",
        )
    });
    fails_naming(dir.path(), "contractVersion");
}

#[test]
fn an_unknown_grammar_revision_fails_naming_the_field() {
    let dir = scratch_root();
    edit(dir.path(), "parity.json", |t| {
        t.replace(
            "6b5433e37b52c03c4c07f9468bf7cc11e45f2f82",
            "0000000000000000000000000000000000000001",
        )
    });
    fails_naming(dir.path(), "formatterGrammarBaseRevision");
}

#[test]
fn a_missing_field_fails_naming_it() {
    let dir = scratch_root();
    edit(dir.path(), "parity.json", |t| {
        t.replace("  \"refusalCaseCount\": 30,\n", "")
    });
    fails_naming(dir.path(), "refusalCaseCount");
}

#[test]
fn a_corpus_path_outside_the_repository_fails() {
    let dir = scratch_root();
    edit(dir.path(), "parity.json", |t| {
        t.replace("tests/fixtures/corpus.json", "../corpus.json")
    });
    fails_naming(dir.path(), "corpusPath");
}
