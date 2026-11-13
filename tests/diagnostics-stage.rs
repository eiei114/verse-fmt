//! Diagnostics slice: every refusal reports the stage that stopped processing,
//! and an adjacent-structure guard refusal also reports the underlying raw parse
//! error. Text output gains a stable `[stage=…]` marker and an opt-in
//! `--explain` block; JSON fills `reason.stage` (and `reason.class`,
//! `reason.underlying`).
//!
//! Fixtures under `tests/fixtures/diagnostics/` are self-authored. They use only
//! public Verse shapes from `docs/parser-design.md`; no private corpus path,
//! content or identifier appears here.
use std::{fs, path::Path, process::Command};

const INLINE_BODY: &str = include_str!("fixtures/diagnostics/inline-binary-body.verse");
const INLINE_BODY_WITH_PARSE_ERROR: &str =
    include_str!("fixtures/diagnostics/inline-binary-body-with-parse-error.verse");
const ADJACENT: &str = include_str!("fixtures/diagnostics/unseparated-adjacent-statements.verse");
const ADJACENT_WITH_PARSE_ERROR: &str =
    include_str!("fixtures/diagnostics/unseparated-adjacent-with-parse-error.verse");

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn put(root: &Path, file: &str, text: &str) -> std::path::PathBuf {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, text).unwrap();
    path
}

fn report(out: &std::process::Output) -> serde_json::Value {
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Every refusal names its stage, in text and in JSON, from the fixed
/// vocabulary. `reader` is the failure this case actually reaches.
#[test]
fn every_refusal_reports_its_stage_in_text_and_json() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let cases = [
        ("lexer.verse", "<# unterminated\n", "lexer"),
        ("delimiter.verse", "A := (1\n", "lexer"),
        ("parse.verse", "Banner:=<p>Text</p>\n", "parser"),
        (
            "guard.verse",
            "Add(Left:int,Right:int):int = Left+Right\n",
            "parser",
        ),
        ("structure.verse", "A()\n  B()\n", "structure"),
        ("source.verse", "A:=1\0\n", "source"),
    ];
    for (file, text, stage) in cases {
        put(root, file, text);

        let out = run(root, &["--check", "--output-format", "json", file]);
        assert_eq!(out.status.code(), Some(2), "{file}: {out:?}");
        let report = report(&out);
        assert_eq!(
            report["files"][0]["reason"]["stage"], stage,
            "{file}: {report}"
        );

        let out = run(root, &["--check", file]);
        assert_eq!(out.status.code(), Some(2), "{file}: {out:?}");
        let text = stderr(&out);
        assert!(text.contains(&format!("[stage={stage}]")), "{file}: {text}");
        assert!(out.stdout.is_empty(), "{file}: {out:?}");
        fs::remove_file(root.join(file)).unwrap();
    }
}

/// A guard refusal is primary and the raw parse error underneath it is
/// secondary: both are reported, at their own offsets, and the guard keeps its
/// refusal wording. This is the slice's core contract.
#[test]
fn a_guard_refusal_also_reports_the_underlying_parse_error() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "case.verse", INLINE_BODY_WITH_PARSE_ERROR);
    let error_offset = INLINE_BODY_WITH_PARSE_ERROR.find("<p>").unwrap();
    let guard_offset = INLINE_BODY_WITH_PARSE_ERROR.find("+Right").unwrap();

    let out = run(root, &["--check", "--output-format", "json", "case.verse"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let report = report(&out);
    let reason = &report["files"][0]["reason"];
    assert_eq!(report["files"][0]["status"], "refused", "{report}");
    assert_eq!(reason["stage"], "parser", "{report}");
    assert_eq!(reason["class"], "guard", "{report}");
    assert_eq!(reason["offset"], guard_offset, "{report}");
    assert!(
        reason["message"]
            .as_str()
            .unwrap()
            .contains("ambiguous adjacent top-level statements"),
        "the guard keeps its wording: {report}"
    );

    let underlying = reason["underlying"].as_array().unwrap();
    assert_eq!(underlying.len(), 1, "{report}");
    assert_eq!(underlying[0]["stage"], "parser", "{report}");
    assert_eq!(underlying[0]["class"], "parse", "{report}");
    assert_eq!(underlying[0]["offset"], error_offset, "{report}");
    assert_eq!(underlying[0]["nodeKind"], "ERROR", "{report}");
    assert!(
        underlying[0]["message"]
            .as_str()
            .unwrap()
            .contains("underlying parse error"),
        "{report}"
    );

    // Text output reports both on stderr, still with the stage marker.
    let out = run(root, &["--check", "case.verse"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let text = stderr(&out);
    assert!(text.contains("[class=guard]"), "{text}");
    assert!(text.contains("[class=parse]"), "{text}");
    assert!(
        text.contains("ambiguous adjacent top-level statements"),
        "{text}"
    );
    assert!(text.contains("underlying parse error"), "{text}");
    // `diagnostic()` adds `verse-fmt: ` once, around the whole message: the
    // secondary line starts with the file label instead of repeating it.
    let secondary = text
        .lines()
        .find(|line| line.contains("underlying parse error"))
        .unwrap();
    assert!(
        secondary.starts_with("case.verse:2:9:"),
        "a secondary line must not repeat the verse-fmt: prefix: {secondary}"
    );
    assert_eq!(text.matches("verse-fmt: ").count(), 1, "{text}");
    assert!(out.stdout.is_empty(), "{out:?}");
}

/// A source the guard refuses with no raw ERROR/MISSING node reports only the
/// guard. A fabricated secondary entry would be a lie about the tree.
#[test]
fn a_guard_only_refusal_reports_no_secondary_entry() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (file, text) in [("inline.verse", INLINE_BODY), ("adjacent.verse", ADJACENT)] {
        put(root, file, text);
        let out = run(root, &["--check", "--output-format", "json", file]);
        assert_eq!(out.status.code(), Some(2), "{file}: {out:?}");
        let report = report(&out);
        let reason = &report["files"][0]["reason"];
        assert_eq!(reason["class"], "guard", "{file}: {report}");
        assert!(
            reason.get("underlying").is_none(),
            "{file}: no secondary entry may be fabricated: {report}"
        );

        let out = run(root, &["--check", "--explain", file]);
        assert_eq!(out.status.code(), Some(2), "{file}: {out:?}");
        let text = stderr(&out);
        assert!(text.contains("failure 1 (primary, parser/guard"), "{text}");
        assert!(!text.contains("failure 2"), "{file}: {text}");
    }
}

/// A raw parse error without the guard is a parse refusal, not a guard one.
#[test]
fn a_parse_error_without_the_guard_is_a_parse_refusal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "case.verse", "Banner:=<p>Text</p>\n");
    let out = run(root, &["--check", "--output-format", "json", "case.verse"]);
    let report = report(&out);
    let reason = &report["files"][0]["reason"];
    assert_eq!(reason["class"], "parse", "{report}");
    assert!(reason.get("underlying").is_none(), "{report}");
}

/// The guard refuses exactly what it refused before. No source becomes accepted
/// because this slice improved the reporting.
#[test]
fn no_source_becomes_accepted_through_the_diagnostics_change() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for (file, text) in [
        ("inline.verse", INLINE_BODY),
        ("inline-parse.verse", INLINE_BODY_WITH_PARSE_ERROR),
        ("adjacent.verse", ADJACENT),
        ("adjacent-parse.verse", ADJACENT_WITH_PARSE_ERROR),
    ] {
        put(root, file, text);
        for mode in ["--check", "--write", "--diff"] {
            let out = run(root, &[mode, file]);
            assert_eq!(out.status.code(), Some(2), "{file} {mode}: {out:?}");
        }
        // `--write` must not have touched the bytes it refused.
        assert_eq!(
            fs::read(root.join(file)).unwrap(),
            text.as_bytes(),
            "{file}"
        );
    }
}

/// `--explain` prints the executed stage order and the whole failure chain;
/// without it the text stays the concise diagnostic it always was.
#[test]
fn explain_prints_the_executed_stages_and_the_full_chain() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "case.verse", INLINE_BODY_WITH_PARSE_ERROR);

    let out = run(root, &["--check", "case.verse"]);
    let plain = stderr(&out);
    assert!(!plain.contains("stages:"), "{plain}");
    assert_eq!(plain.lines().count(), 2, "{plain}");

    let out = run(root, &["--check", "--explain", "case.verse"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let explained = stderr(&out);
    assert!(
        explained.contains("stages: source -> lexer -> parser"),
        "{explained}"
    );
    assert!(
        explained.contains("failure 1 (primary, parser/guard"),
        "{explained}"
    );
    assert!(
        explained.contains("failure 2 (secondary, parser/parse, ERROR"),
        "{explained}"
    );
    assert!(explained.contains("offset"), "{explained}");
    assert!(
        explained.starts_with(&plain),
        "the concise diagnostic stays: {explained}"
    );
    assert!(out.stdout.is_empty(), "{out:?}");

    // A stage that stops earlier reports only the stages it entered.
    put(root, "lexer.verse", "<# unterminated\n");
    let out = run(root, &["--check", "--explain", "lexer.verse"]);
    let explained = stderr(&out);
    assert!(explained.contains("stages: source -> lexer"), "{explained}");
}

/// `--explain` with `--output-format json` is rejected with exit 2 and nothing
/// on stdout: the envelope already carries `reason.stage`, `reason.class` and
/// `reason.underlying`, so the flag would only duplicate them.
#[test]
fn explain_with_json_is_rejected_with_exit_two() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A:=1\n");
    let out = run(root, &["--explain", "--output-format", "json", "a.verse"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert!(stderr(&out).contains("--explain is text only"), "{out:?}");
}

/// An `--explain` stage list always ends at the stage that stopped processing,
/// for the later stages the binary can actually reach. A structure failure
/// happens while the trace tail is `structure` and a layout failure while it is
/// `layout`, so each must print that stage last rather than the reparse's
/// `parser`.
#[test]
fn explain_stage_lists_end_at_the_stage_that_stopped_processing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    // `A()` followed by a deeper `B()` line is neither a block introducer nor a
    // supported continuation, so `structure` is the stage that stops processing.
    for (file, text) in [
        ("tab.verse", "f():void =\n\tA()\n"),
        ("unsupported.verse", "A()\n  B()\n"),
    ] {
        put(root, file, text);
        let out = run(root, &["--check", "--explain", file]);
        assert_eq!(out.status.code(), Some(2), "{file}: {out:?}");
        let text = stderr(&out);
        assert!(text.contains("[stage=structure]"), "{file}: {text}");
        assert!(
            text.contains("stages: source -> lexer -> parser -> structure"),
            "{file}: {text}"
        );
        assert!(out.stdout.is_empty(), "{file}: {out:?}");
    }

    // A forced line ending that would rewrite a protected span stops in
    // `layout`, the last stage the source entered before the failure.
    put(root, "verse.toml", "[format]\nline-ending = \"lf\"\n");
    put(root, "protected.verse", "<# mixed\r\nLF\n#>\nA:=1\n");
    let out = run(root, &["--check", "--explain", "protected.verse"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let text = stderr(&out);
    assert!(text.contains("[stage=layout]"), "{text}");
    assert!(
        text.contains("stages: source -> lexer -> parser -> structure -> layout"),
        "{text}"
    );
    assert!(out.stdout.is_empty(), "{out:?}");
}

/// A source that only needs formatting keeps its current behaviour: no stage
/// marker on the success path, no explain block, unchanged exit codes.
#[test]
fn successful_runs_keep_their_existing_output() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A:=1\n");
    for args in [
        vec!["--check", "a.verse"],
        vec!["--check", "--explain", "a.verse"],
    ] {
        let out = run(root, &args);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {out:?}");
        assert!(!stderr(&out).contains("stage="), "{args:?}: {out:?}");
        assert_eq!(out.stdout, b"a.verse\n", "{args:?}");
    }

    let out = run(root, &["--check", "--output-format", "json", "a.verse"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let report = report(&out);
    assert_eq!(report["files"][0]["reason"]["stage"], "layout");
    assert!(report["files"][0]["reason"].get("class").is_none());
    assert!(report["files"][0]["reason"].get("underlying").is_none());
}
