//! In-repository coverage for every documented resource limit.
//!
//! The canonical list is the resource-limit sentence in the maintainer vault's
//! `Docs/windows-toolchain-contract.md`; this repository mirrors it in
//! `docs/cli.md#resource-limits-and-the-tests-that-enforce-them`, which names the
//! test for each limit. Before this file, most of these values were only covered
//! by one-off runs recorded in project history — a regression would have gone
//! unnoticed.
//!
//! Every limit a real input can reach cheaply is exercised end to end through the
//! CLI: at the limit it is accepted, one past it exits 2 with an execution error
//! (`complete: false`), and `--write` replaces no file. Limits whose literal
//! value cannot be reached under the other limits (100,000 visited entries,
//! 1,000,000 fingerprint events, the two-second parse budget) are proven in-crate
//! by invoking the predicate or a reduced test seam directly; each such test
//! documents the technique and is named in the docs table.
//!
//! The constants below repeat the documented values because this package is a
//! binary crate and an integration test cannot read its private constants. The
//! drift guard in slice 04 compares the documented sentence against the constants
//! themselves, so a changed limit cannot silently disagree with this table.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::SystemTime,
};

const MIB: usize = 1024 * 1024;
const MAX_SOURCE_BYTES: usize = 8 * MIB;
const MAX_CONFIG_BYTES: usize = 256 * 1024;
const MAX_EXCLUDE_GLOBS: usize = 256;
const MAX_EXCLUDE_GLOB_BYTES: usize = 4_096;
/// Directory levels counted from the config root.
const MAX_DEPTH_LEVELS: usize = 128;
/// A line `A\n` is two lexical tokens (code plus newline), so this many lines
/// reach the documented 250,000-token cap exactly.
const MAX_TOKEN_LINES: usize = 250_000 / 2;
/// A chain of 510 `+ 1` additions measures CST depth 512, the documented bound;
/// one more addition reaches 513 and is refused.
const MAX_CHAIN_ADDITIONS: usize = 510;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn put(root: &Path, file: &str, bytes: &[u8]) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Parse the envelope, requiring that stdout is exactly one JSON object.
fn report(out: &Output) -> serde_json::Value {
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(
        text.starts_with('{') && text.ends_with('}'),
        "stdout is not one envelope: {text}"
    );
    serde_json::from_str(&text).unwrap()
}

fn status<'a>(report: &'a serde_json::Value, path: &str) -> Option<&'a str> {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == path)?
        .get("status")?
        .as_str()
}

fn reason_stage<'a>(report: &'a serde_json::Value, path: &str) -> Option<&'a str> {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == path)?
        .get("reason")?
        .get("stage")?
        .as_str()
}

/// `(path, bytes, mtime)` for every file under `root`: enough to prove that a
/// refused invocation wrote nothing, including "did not rewrite identical
/// bytes".
fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = fs::read(&path).unwrap();
                let modified = fs::metadata(&path).unwrap().modified().unwrap();
                files.push((path, bytes, modified));
            }
        }
    }
    files.sort();
    files
}

/// A counted refusal: exit 2, `complete: false`, and at least one execution
/// error. A limit failure is an execution error, never a formatting outcome:
/// no entry may be `formatted`, because nothing was written.
fn assert_refused(out: &Output, what: &str) -> serde_json::Value {
    assert_eq!(out.status.code(), Some(2), "{what}: {out:?}");
    let report = report(out);
    assert_eq!(report["summary"]["complete"], false, "{what}: {report}");
    assert!(
        !report["executionErrors"].as_array().unwrap().is_empty(),
        "{what}: refusal without an execution error: {report}"
    );
    for file in report["files"].as_array().unwrap() {
        assert_ne!(
            file["status"].as_str().unwrap(),
            "formatted",
            "{what}: a refused run wrote a file: {report}"
        );
    }
    report
}

fn assert_accepted(out: &Output, what: &str) -> serde_json::Value {
    assert_ne!(out.status.code(), Some(2), "{what}: {out:?}");
    let report = report(out);
    assert_eq!(report["summary"]["complete"], true, "{what}: {report}");
    assert_eq!(
        report["executionErrors"].as_array().unwrap().len(),
        0,
        "{what}: {report}"
    );
    report
}

/// The per-source byte limit: exactly 8 MiB is analyzed, one byte more is a
/// `source` refusal that also blocks the write of a neighbouring change.
#[test]
fn source_byte_limit_is_enforced_at_eight_mib() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    // Spaces are the cheapest valid 8 MiB payload: one lexical token, no lines.
    put(root, "at.verse", &vec![b' '; MAX_SOURCE_BYTES]);
    let report = assert_accepted(
        &run(root, &["at.verse", "--check", "--output-format", "json"]),
        "8 MiB source at the limit",
    );
    assert_eq!(status(&report, "at.verse"), Some("would-format"));
    assert_eq!(reason_stage(&report, "at.verse"), Some("layout"));

    put(root, "a-change.verse", b"A:=1\n");
    put(root, "z-oversized.verse", &vec![b' '; MAX_SOURCE_BYTES + 1]);
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "8 MiB source one byte past");
    assert_eq!(status(&report, "z-oversized.verse"), Some("refused"));
    assert_eq!(reason_stage(&report, "z-oversized.verse"), Some("source"));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("source exceeds the 8 MiB limit"),
        "{out:?}"
    );
    // The whole batch is preflighted before the first replacement, so the file
    // that could have been formatted was left untouched.
    assert_eq!(snapshot(root), before);
    assert_eq!(fs::read(root.join("a-change.verse")).unwrap(), b"A:=1\n");
}

/// The aggregate input limit: 64 MiB exactly is analyzed; one byte more is a
/// `source` refusal, and the files already read keep their reported outcome.
#[test]
fn total_input_limit_is_enforced_at_sixty_four_mib() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for index in 0..8 {
        put(
            root,
            &format!("file-{index:02}.verse"),
            &vec![b' '; MAX_SOURCE_BYTES],
        );
    }
    let report = assert_accepted(
        &run(root, &[".", "--check", "--output-format", "json"]),
        "64 MiB total input at the limit",
    );
    assert_eq!(report["summary"]["filesChecked"], 8, "{report}");
    assert_eq!(report["summary"]["filesChanged"], 8, "{report}");

    // The 64 MiB + 2 byte file sorts last, so the aggregate crosses the limit
    // only after all eight files were analyzed.
    put(root, "zz-extra.verse", b"A\n");
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "64 MiB total input one byte past");
    assert_eq!(status(&report, "zz-extra.verse"), Some("refused"));
    assert_eq!(reason_stage(&report, "zz-extra.verse"), Some("source"));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("inputs exceed the 64 MiB total limit"),
        "{out:?}"
    );
    // Partial completion: the eight already-read sources still report their own
    // result instead of being discarded with the failing one.
    assert_eq!(report["summary"]["filesChecked"], 9, "{report}");
    assert_eq!(
        report["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|file| file["status"] == "would-format")
            .count(),
        8,
        "{report}"
    );
    assert_eq!(snapshot(root), before);
}

/// The lexical nesting limit: 256 levels parse, 257 is a `lexer` refusal.
#[test]
fn lexical_nesting_limit_is_enforced_at_256() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let nested = |levels: usize| {
        let mut text = String::from("A := ");
        text.push_str(&"(".repeat(levels));
        text.push('1');
        text.push_str(&")".repeat(levels));
        text.push('\n');
        text
    };
    put(root, "at.verse", nested(256).as_bytes());
    assert_accepted(
        &run(root, &["at.verse", "--check", "--output-format", "json"]),
        "nesting at 256",
    );

    put(root, "a-change.verse", b"A:=1\n");
    put(root, "z-deep.verse", nested(257).as_bytes());
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "nesting one level past");
    assert_eq!(status(&report, "z-deep.verse"), Some("refused"));
    assert_eq!(reason_stage(&report, "z-deep.verse"), Some("lexer"));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nesting exceeds 256 levels"),
        "{out:?}"
    );
    assert_eq!(snapshot(root), before);
}

/// The directory-depth limit. A source reached after 127 hops sits at level 128
/// and is discovered; 128 hops put it at level 129 and stop the walk.
///
/// Past the limit the run still reports the sources discovered before it, which
/// is the documented partial-completion contract: the limit is an execution
/// error for the invocation, not a reason to discard an already analyzed file.
#[test]
fn directory_depth_limit_is_enforced_at_128() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "ok.verse", b"A:=1\n");
    let mut at_limit = root.to_path_buf();
    for _ in 0..MAX_DEPTH_LEVELS - 1 {
        at_limit = at_limit.join("x");
        fs::create_dir(&at_limit).unwrap();
    }
    fs::write(at_limit.join("deep.verse"), b"D:=9\n").unwrap();

    let report = assert_accepted(
        &run(root, &[".", "--check", "--output-format", "json"]),
        "directory depth at 128",
    );
    assert_eq!(report["summary"]["filesChecked"], 2, "{report}");
    assert_eq!(
        status(
            &report,
            &format!("{}deep.verse", "x/".repeat(MAX_DEPTH_LEVELS - 1))
        ),
        Some("would-format"),
        "{report}"
    );

    put(root, "z-after.verse", b"Z:=7\n");
    let mut past = root.to_path_buf();
    for _ in 0..MAX_DEPTH_LEVELS {
        past = past.join("y");
        fs::create_dir(&past).unwrap();
    }
    fs::write(past.join("deep.verse"), b"D:=9\n").unwrap();
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "directory depth one level past");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("directory traversal resource limit"),
        "{out:?}"
    );
    // Partial completion: the walk stopped, so the two sources found before the
    // limit (the root file and the level-128 file) still report their own result.
    // The limit stays an invocation-level execution error (`path: null`,
    // `complete: false`) and any refusal still blocks the write phase, so
    // nothing is written.
    assert_eq!(report["summary"]["filesChecked"], 2, "{report}");
    assert_eq!(report["summary"]["filesRefused"], 0, "{report}");
    assert_eq!(
        status(&report, "ok.verse"),
        Some("would-format"),
        "{report}"
    );
    assert_eq!(
        status(
            &report,
            &format!("{}deep.verse", "x/".repeat(MAX_DEPTH_LEVELS - 1))
        ),
        Some("would-format"),
        "{report}"
    );
    assert_eq!(status(&report, "z-after.verse"), None, "{report}");
    assert_eq!(
        report["executionErrors"][0]["path"],
        serde_json::Value::Null,
        "{report}"
    );
    assert_eq!(snapshot(root), before);
    assert_eq!(fs::read(root.join("ok.verse")).unwrap(), b"A:=1\n");
}

/// The configuration byte limit: a 256 KiB `verse.toml` resolves, one byte more
/// is a `discovery` refusal and no file is written.
#[test]
fn config_byte_limit_is_enforced_at_256_kib() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "ok.verse", b"A:=1\n");
    // A TOML comment is valid configuration and reaches the byte limit without
    // adding a setting this test would have to reason about.
    put(root, "verse.toml", &comment_of(MAX_CONFIG_BYTES));
    let report = assert_accepted(
        &run(root, &[".", "--check", "--output-format", "json"]),
        "config at 256 KiB",
    );
    assert_eq!(status(&report, "ok.verse"), Some("would-format"));

    put(root, "verse.toml", &comment_of(MAX_CONFIG_BYTES + 1));
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "config one byte past");
    assert_eq!(report["summary"]["filesChecked"], 0, "{report}");
    assert_eq!(
        report["executionErrors"][0]["path"],
        serde_json::Value::Null,
        "{report}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("configuration exceeds 256 KiB"),
        "{out:?}"
    );
    assert_eq!(snapshot(root), before);
}

/// The local ignore-file byte limit, on the same boundary as `verse.toml`.
#[test]
fn gitignore_byte_limit_is_enforced_at_256_kib() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "ok.verse", b"A:=1\n");
    put(root, ".gitignore", &comment_of(MAX_CONFIG_BYTES));
    let report = assert_accepted(
        &run(root, &[".", "--check", "--output-format", "json"]),
        "gitignore at 256 KiB",
    );
    assert_eq!(status(&report, "ok.verse"), Some("would-format"));

    put(root, ".gitignore", &comment_of(MAX_CONFIG_BYTES + 1));
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "gitignore one byte past");
    assert_eq!(report["summary"]["filesChecked"], 0, "{report}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(".gitignore exceeds 256 KiB"),
        "{out:?}"
    );
    assert_eq!(snapshot(root), before);
}

/// The exclude-glob limits: 256 patterns are accepted and a 4,096-byte pattern
/// is accepted, while 257 patterns and a 4,097-byte pattern are `discovery`
/// refusals.
///
/// The two limits are tested separately rather than together: 256 x 4,096 bytes
/// would be 1 MiB of configuration, and the 256 KiB `verse.toml` limit refuses
/// that first. They cannot be reached at once, which is a real interaction
/// between two documented limits and is reported in the pull request.
#[test]
fn exclude_glob_limits_are_enforced_at_256_globs_and_4096_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "ok.verse", b"A:=1\n");

    let pattern = |index: usize, bytes: usize| {
        let mut pattern = format!("pattern-{index}/");
        pattern.push_str(&"a".repeat(bytes - pattern.len()));
        pattern
    };

    // 256 globs at the documented count.
    put(
        root,
        "verse.toml",
        &glob_config((0..MAX_EXCLUDE_GLOBS).map(|index| pattern(index, 32))),
    );
    let report = assert_accepted(
        &run(root, &[".", "--check", "--output-format", "json"]),
        "256 exclude globs",
    );
    assert_eq!(status(&report, "ok.verse"), Some("would-format"));

    // One glob at the documented byte size.
    put(
        root,
        "verse.toml",
        &glob_config(std::iter::once(pattern(0, MAX_EXCLUDE_GLOB_BYTES))),
    );
    let report = assert_accepted(
        &run(root, &[".", "--check", "--output-format", "json"]),
        "a 4,096-byte exclude glob",
    );
    assert_eq!(status(&report, "ok.verse"), Some("would-format"));

    // One byte too long.
    put(
        root,
        "verse.toml",
        &glob_config(std::iter::once(pattern(0, MAX_EXCLUDE_GLOB_BYTES + 1))),
    );
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "glob one byte past 4,096");
    assert_eq!(report["summary"]["filesChecked"], 0, "{report}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("invalid exclude glob"),
        "{out:?}"
    );
    assert_eq!(snapshot(root), before);

    // One glob too many.
    put(
        root,
        "verse.toml",
        &glob_config((0..MAX_EXCLUDE_GLOBS + 1).map(|index| pattern(index, 32))),
    );
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "one glob past 256");
    assert_eq!(report["summary"]["filesChecked"], 0, "{report}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("at most 256 exclude globs"),
        "{out:?}"
    );
    assert_eq!(snapshot(root), before);
}

/// The lexical token limit: exactly 250,000 tokens are analyzed; the 250,001st
/// is a `lexer` refusal.
#[test]
fn token_limit_is_enforced_at_250000() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "at.verse", &tokens(MAX_TOKEN_LINES));
    assert_accepted(
        &run(root, &["at.verse", "--check", "--output-format", "json"]),
        "250,000 tokens at the limit",
    );

    put(root, "a-change.verse", b"A:=1\n");
    let mut past = tokens(MAX_TOKEN_LINES);
    past.push(b'A');
    put(root, "z-tokens.verse", &past);
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "token limit one past");
    assert_eq!(status(&report, "z-tokens.verse"), Some("refused"));
    assert_eq!(reason_stage(&report, "z-tokens.verse"), Some("lexer"));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("source exceeds 250000 lexical tokens"),
        "{out:?}"
    );
    assert_eq!(snapshot(root), before);
}

/// The CST depth limit: the deepest accepted chain measures 512; one more
/// addition reaches 513 and is a `parser` refusal.
#[test]
fn cst_depth_limit_is_enforced_at_512() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "at.verse", &chain(MAX_CHAIN_ADDITIONS));
    assert_accepted(
        &run(root, &["at.verse", "--check", "--output-format", "json"]),
        "CST depth at 512",
    );

    put(root, "a-change.verse", b"A:=1\n");
    put(root, "z-deep.verse", &chain(MAX_CHAIN_ADDITIONS + 1));
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "CST depth one past");
    assert_eq!(status(&report, "z-deep.verse"), Some("refused"));
    assert_eq!(reason_stage(&report, "z-deep.verse"), Some("parser"));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("syntax tree resource limit exceeded"),
        "{out:?}"
    );
    assert_eq!(snapshot(root), before);
}

/// The discovery error cap, `MAX_DISCOVERY_ERRORS` in `src/files.rs`.
///
/// At the limit: 100 missing inputs are all reported, so the cap is not reached
/// early. Past the limit: a run with 150 missing inputs still reports exactly
/// 100 path diagnostics — the walk stops collecting, so the envelope stays
/// bounded instead of growing with the input count.
#[test]
fn discovery_error_cap_is_enforced_at_100() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "ok.verse", b"A:=1\n");

    let mut args: Vec<String> = vec![
        ".".into(),
        "--check".into(),
        "--output-format".into(),
        "json".into(),
    ];
    args.extend((0..100).map(|index| format!("missing-{index}.verse")));
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let report = assert_refused(&run(root, &borrowed), "100 missing inputs");
    assert_eq!(
        report["executionErrors"].as_array().unwrap().len(),
        100,
        "{report}"
    );

    let mut args: Vec<String> = vec![
        ".".into(),
        "--write".into(),
        "--output-format".into(),
        "json".into(),
    ];
    args.extend((0..150).map(|index| format!("gone-{index}.verse")));
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
    let before = snapshot(root);
    let report = assert_refused(&run(root, &borrowed), "150 missing inputs");
    assert_eq!(
        report["executionErrors"].as_array().unwrap().len(),
        100,
        "{report}"
    );
    assert_eq!(snapshot(root), before);
}

/// The source-count limit: 10,000 sources are analyzed, the 10,001st stops the
/// walk as a `discovery` refusal. The 10,000 already discovered still report
/// their own result (documented partial completion); the count itself is an
/// invocation-level execution error, and nothing is written.
#[test]
fn source_count_limit_is_enforced_at_10000() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for index in 0..MAX_SOURCES {
        put(
            root,
            &format!("source-{index:05}.verse"),
            if index == 0 { b"A:=1\n" } else { b"" },
        );
    }
    let report = assert_accepted(
        &run(root, &[".", "--check", "--output-format", "json"]),
        "10,000 sources at the limit",
    );
    assert_eq!(report["summary"]["filesChecked"], MAX_SOURCES, "{report}");

    put(root, "zz-extra.verse", b"A:=1\n");
    put(root, "zzz-later.verse", b"Z:=7\n");
    let before = snapshot(root);
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    let report = assert_refused(&out, "10,001st source");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("at most 10000 source files"),
        "{out:?}"
    );
    // Partial completion: every source discovered before the limit keeps its
    // own result instead of being thrown away with the refusal.
    assert_eq!(report["summary"]["filesChecked"], MAX_SOURCES, "{report}");
    assert_eq!(report["summary"]["filesRefused"], 0, "{report}");
    assert_eq!(status(&report, "source-00000.verse"), Some("would-format"));
    assert_eq!(status(&report, "zzz-later.verse"), None, "{report}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr)
            .matches("at most 10000 source files")
            .count(),
        1,
        "{out:?}"
    );
    assert_eq!(
        report["executionErrors"][0]["path"],
        serde_json::Value::Null,
        "{report}"
    );
    assert_eq!(snapshot(root), before);
    assert_eq!(
        fs::read(root.join("source-00000.verse")).unwrap(),
        b"A:=1\n"
    );
}

const MAX_SOURCES: usize = 10_000;

fn comment_of(bytes: usize) -> Vec<u8> {
    let mut text = String::with_capacity(bytes);
    text.push('#');
    text.push_str(&"c".repeat(bytes - 2));
    text.push('\n');
    text.into_bytes()
}

fn glob_config(patterns: impl Iterator<Item = String>) -> Vec<u8> {
    let list = patterns
        .map(|pattern| format!("{pattern:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[files]\nexclude = [{list}]\n").into_bytes()
}

fn tokens(lines: usize) -> Vec<u8> {
    let mut text = String::with_capacity(lines * 2);
    for _ in 0..lines {
        text.push_str("A\n");
    }
    text.into_bytes()
}

fn chain(additions: usize) -> Vec<u8> {
    let mut text = String::from("A := 1");
    for _ in 0..additions {
        text.push_str(" + 1");
    }
    text.push('\n');
    text.into_bytes()
}
