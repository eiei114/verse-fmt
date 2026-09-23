use serde::Deserialize;
use std::{
    collections::HashSet,
    io::Write,
    process::{Command, Stdio},
};
#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    category: String,
    input: String,
    expected: Option<String>,
    #[serde(default)]
    reject: bool,
}
fn format(bytes: &[u8], root: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}
#[test]
fn manifest_golden_rejection_and_idempotence_cases() {
    let corpus: Corpus = serde_json::from_str(include_str!("fixtures/corpus.json")).unwrap();
    assert!(corpus.cases.len() >= 60);
    let dir = tempfile::tempdir().unwrap();
    let mut ids = HashSet::new();
    let mut failures = Vec::new();
    for case in corpus.cases {
        assert!(ids.insert(case.id.clone()), "duplicate case ID");
        assert!(!case.category.is_empty());
        let out = format(case.input.as_bytes(), dir.path());
        if case.reject {
            if out.status.code() != Some(2) || !out.stdout.is_empty() {
                failures.push(format!("{} expected refusal: {out:?}", case.id));
            }
        } else {
            let expected = case
                .expected
                .expect("positive case has independently specified golden");
            if out.status.code() != Some(0) || out.stdout != expected.as_bytes() {
                failures.push(format!(
                    "{} expected {:?}, actual {:?}; {}",
                    case.id,
                    expected,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                ));
                continue;
            }
            let twice = format(&out.stdout, dir.path());
            assert!(twice.status.success(), "{}", case.id);
            assert_eq!(twice.stdout, out.stdout, "{} idempotence", case.id);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
