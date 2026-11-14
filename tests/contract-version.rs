//! The shared toolchain contract version.
//!
//! `verse-fmt` and `verse-lint` are one pair: both documents assert the same
//! contract version, and a change to CLI, configuration or exit-code behaviour
//! has to move it in both repositories together. This repository cannot read the
//! sibling checkout in CI, so `tests/fixtures/contract-pair.json` records the
//! value the linter asserts (with the commit it was read at), and
//! `VERSE_LINT_DOC` re-reads that file live when a developer has it checked out.
//! Either way a divergence fails here instead of surfacing in the other repository
//! later.

use std::fs;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContractPair {
    contract_version: u32,
    formatter_doc: String,
    linter_contract_version: u32,
    linter_repository: String,
    linter_doc: String,
    #[serde(default)]
    linter_observed_commit: Option<String>,
}

/// The version a document asserts, in either repository's wording:
/// `Shared toolchain contract version **1**.` or
/// `Shared toolchain contract version: **1** (not the same as ...)`.
fn asserted_version(text: &str) -> Option<u32> {
    let marker = "Shared toolchain contract version";
    let start = text.find(marker)? + marker.len();
    let rest = &text[start..];
    let open = rest.find("**")? + 2;
    let close = rest[open..].find("**")? + open;
    rest[open..close].trim().parse().ok()
}

fn pair() -> ContractPair {
    serde_json::from_str(include_str!("fixtures/contract-pair.json"))
        .expect("tests/fixtures/contract-pair.json must stay parseable")
}

#[test]
fn the_declared_pair_matches_this_repository() {
    let pair = pair();
    let cli_docs = include_str!("../docs/cli.md");
    // The value is read from the record, never written into this test: a
    // coordinated move of both documents and contract-pair.json must keep
    // passing, while a document that stops carrying the version sentence fails.
    let expected_sentence = format!(
        "Shared toolchain contract version **{}**.",
        pair.contract_version
    );
    assert!(
        cli_docs.contains(&expected_sentence),
        "{} does not state `{expected_sentence}`, which tests/fixtures/contract-pair.json \
         records as contractVersion {}",
        pair.formatter_doc,
        pair.contract_version
    );
    let formatter = asserted_version(cli_docs).unwrap_or_else(|| {
        panic!(
            "{} does not assert a `Shared toolchain contract version **N**` value",
            pair.formatter_doc
        )
    });
    assert_eq!(
        formatter, pair.contract_version,
        "tests/fixtures/contract-pair.json records contractVersion {}, but {} asserts {formatter}",
        pair.contract_version, pair.formatter_doc
    );
    assert_eq!(
        pair.contract_version, pair.linter_contract_version,
        "the pair must assert one shared toolchain contract version: this repository asserts {}, \
         but tests/fixtures/contract-pair.json records {} asserting {}",
        pair.contract_version, pair.linter_repository, pair.linter_contract_version
    );
    if let Some(commit) = &pair.linter_observed_commit {
        assert!(
            commit.len() >= 7 && commit.chars().all(|ch| ch.is_ascii_hexdigit()),
            "tests/fixtures/contract-pair.json records the linter version at {commit:?}, \
             which is not a commit hash"
        );
    }
}

/// A divergence between the two repositories must fail here. Without
/// `VERSE_LINT_DOC` the recorded value is compared; with it, the sibling document
/// is re-read so a stale record cannot hide a real divergence.
#[test]
fn a_contract_version_divergence_between_the_repositories_fails() {
    let pair = pair();
    let formatter = asserted_version(include_str!("../docs/cli.md")).unwrap();
    let Ok(path) = std::env::var("VERSE_LINT_DOC") else {
        return;
    };
    let linter_doc = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read VERSE_LINT_DOC {path}: {error}"));
    let linter = asserted_version(&linter_doc).unwrap_or_else(|| {
        panic!(
            "{path} ({}) does not assert a `Shared toolchain contract version **N**` value",
            pair.linter_doc
        )
    });
    assert_eq!(
        formatter, linter,
        "{}({}) asserts contract version {formatter}, but {}({}) asserts {linter}; \
         the shared contract version must move in both repositories together",
        pair.formatter_doc, pair.contract_version, pair.linter_repository, linter
    );
    assert_eq!(
        linter, pair.linter_contract_version,
        "{path} asserts contract version {linter}, but tests/fixtures/contract-pair.json \
         records {}; update the record when the sibling document moves",
        pair.linter_contract_version
    );
}
