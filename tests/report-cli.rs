//! `--output-format json` contract: envelope shape, status mapping, schema
//! validity, determinism and the text-path compatibility promise.
use std::{fs, path::Path, process::Command};

fn schema() -> serde_json::Value {
    serde_json::from_str(include_str!("../docs/diagnostics.schema.json")).unwrap()
}

/// Minimal draft-07 subset validator: the schema uses only `type`, `required`,
/// `properties`, `additionalProperties`, `enum`, `const`, `items`, `minimum`,
/// `minLength`, `$ref`, `if`/`then`/`else` and `not`. Keeping the check in-tree
/// means the schema is enforced on every `cargo test` without another service.
fn validate(
    value: &serde_json::Value,
    schema: &serde_json::Value,
    root: &serde_json::Value,
) -> Vec<String> {
    let schema = match schema.get("$ref").and_then(|r| r.as_str()) {
        Some(reference) => {
            let pointer = reference.trim_start_matches("#/");
            pointer
                .split('/')
                .fold(root, |node, key| &node[key])
                .clone()
        }
        None => schema.clone(),
    };
    let mut errors = Vec::new();
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        errors.push(format!("expected const {expected}, got {value}"));
    }
    if let Some(values) = schema.get("enum").and_then(|e| e.as_array())
        && !values.contains(value)
    {
        errors.push(format!("{value} is not one of {values:?}"));
    }
    if let Some(kind) = schema.get("type") {
        let kinds: Vec<&str> = match kind {
            serde_json::Value::String(s) => vec![s.as_str()],
            other => other
                .as_array()
                .unwrap()
                .iter()
                .map(|k| k.as_str().unwrap())
                .collect(),
        };
        let matches = kinds.iter().any(|kind| match *kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            other => panic!("unsupported schema type {other}"),
        });
        if !matches {
            errors.push(format!("{value} is not of type {kinds:?}"));
            return errors;
        }
    }
    if let Some(minimum) = schema.get("minimum").and_then(|m| m.as_i64())
        && value.as_i64().is_some_and(|n| n < minimum)
    {
        errors.push(format!("{value} is below the minimum {minimum}"));
    }
    if let Some(minimum) = schema.get("minLength").and_then(|m| m.as_u64())
        && value
            .as_str()
            .is_some_and(|s| s.chars().count() < minimum as usize)
    {
        errors.push(format!("{value} is shorter than {minimum}"));
    }
    if let Some(items) = schema.get("items")
        && let Some(array) = value.as_array()
    {
        for item in array {
            errors.extend(validate(item, items, root));
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for key in required {
                let key = key.as_str().unwrap();
                if !object.contains_key(key) {
                    errors.push(format!("missing required property {key:?}"));
                }
            }
        }
        if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
            for (key, child) in object {
                match properties.get(key) {
                    Some(property) => errors.extend(validate(child, property, root)),
                    None if schema.get("additionalProperties")
                        == Some(&serde_json::Value::Bool(false)) =>
                    {
                        errors.push(format!("unexpected property {key:?}"));
                    }
                    None => (),
                }
            }
        }
        if let (Some(condition), Some(then), Some(otherwise)) =
            (schema.get("if"), schema.get("then"), schema.get("else"))
        {
            let branch = if validate(value, condition, root).is_empty() {
                then
            } else {
                otherwise
            };
            errors.extend(validate(value, branch, root));
        }
        if let Some(branch) = schema.get("not")
            && validate(value, branch, root).is_empty()
        {
            errors.push("value matches a forbidden shape".into());
        }
    }
    errors
}

fn assert_valid(envelope: &str) {
    let schema = schema();
    let value: serde_json::Value = serde_json::from_str(envelope).unwrap();
    let errors = validate(&value, &schema, &schema);
    assert!(
        errors.is_empty(),
        "schema violations: {errors:?}\n{envelope}"
    );
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn put(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn envelope(out: &std::process::Output) -> serde_json::Value {
    let text = String::from_utf8(out.stdout.clone()).unwrap();
    assert_valid(&text);
    serde_json::from_str(&text).unwrap()
}

#[test]
fn reports_unchanged_would_format_formatted_and_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "ok.verse", "A := 1\n");
    put(root, "needs.verse", "B:=2\n");
    put(root, "broken.verse", "<# unterminated\n");

    let out = run(root, &[".", "--check", "--output-format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let report = envelope(&out);
    assert_eq!(report["schemaVersion"], 1);
    assert_eq!(report["tool"]["name"], "verse-fmt");
    assert_eq!(report["tool"]["version"], env!("CARGO_PKG_VERSION"));
    let files = report["files"].as_array().unwrap();
    assert_eq!(files.len(), 3, "{report}");
    let status = |path: &str| {
        files.iter().find(|file| file["path"] == path).unwrap()["status"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(status("ok.verse"), "unchanged");
    assert_eq!(status("needs.verse"), "would-format");
    assert_eq!(status("broken.verse"), "refused");
    assert_eq!(report["summary"]["filesChecked"], 3);
    assert_eq!(report["summary"]["filesChanged"], 1);
    assert_eq!(report["summary"]["filesUnchanged"], 1);
    assert_eq!(report["summary"]["filesRefused"], 1);
    assert_eq!(report["summary"]["complete"], false);
    assert_eq!(report["executionErrors"].as_array().unwrap().len(), 1);
    assert_eq!(report["executionErrors"][0]["path"], "broken.verse");

    let out = run(root, &[".", "--write", "--output-format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    fs::remove_file(root.join("broken.verse")).unwrap();
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let report = envelope(&out);
    assert_eq!(report["files"][0]["path"], "needs.verse");
    assert_eq!(report["files"][0]["status"], "formatted");
    assert_eq!(report["files"][1]["path"], "ok.verse");
    assert_eq!(report["files"][1]["status"], "unchanged");
    assert_eq!(report["summary"]["complete"], true);
    assert!(report["files"][1].get("reason").is_none(), "{report}");
}

#[test]
fn machine_output_is_deterministic_and_carries_no_human_text() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "nested 日本語/device.verse", "A:=1\n");
    let first = run(root, &[".", "--check", "--output-format", "json"]);
    let second = run(
        root,
        &[
            "--color",
            "always",
            ".",
            "--check",
            "--output-format",
            "json",
        ],
    );
    assert_eq!(first.stdout, second.stdout);
    let text = String::from_utf8(first.stdout).unwrap();
    assert!(!text.contains('\u{1b}'), "{text}");
    assert!(!text.contains(root.to_str().unwrap()), "{text}");
    assert!(!text.contains('\\'), "{text}");
    assert_eq!(first.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["files"][0]["path"], "nested 日本語/device.verse");
    assert_eq!(report["files"][0]["reason"]["stage"], "layout");
}

#[test]
fn conflicting_combinations_fail_without_emitting_json() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A:=1\n");
    for args in [
        vec![".", "--diff", "--output-format", "json"],
        vec!["--show-config", "--output-format", "json"],
    ] {
        let out = run(root, &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("cannot be combined"),
            "{args:?}"
        );
    }
}

#[test]
fn text_format_stays_the_default_and_matches_the_explicit_flag() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A:=1\n");
    for args in [
        vec![".", "--check"],
        vec!["."],
        vec!["a.verse"],
        vec![".", "--diff"],
        vec!["--show-config"],
        vec![".", "--check", "--verbose"],
        vec!["missing.verse"],
    ] {
        let bare = run(root, &args);
        let mut explicit = vec!["--output-format", "text"];
        explicit.extend(args.iter().copied());
        let flagged = run(root, &explicit);
        assert_eq!(bare.stdout, flagged.stdout, "{args:?}");
        assert_eq!(bare.stderr, flagged.stderr, "{args:?}");
        assert_eq!(bare.status.code(), flagged.status.code(), "{args:?}");
    }
}

#[test]
fn refusals_map_to_the_stage_that_actually_decided() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    // Every stage the binary can honestly attribute today. `validation` is a
    // defensive internal guard that no known input reaches; `discovery`,
    // `write` and `reporting` are reserved by the shared contract.
    let cases = [
        ("lexer.verse", "<# unterminated\n", "lexer"),
        ("delimiter.verse", "A := (1\n", "lexer"),
        ("parser.verse", "Text:=<p>Hello</p>\n", "parser"),
        ("structure.verse", "A()\n  B()\n", "structure"),
        ("source.verse", "A:=1\0\n", "source"),
        ("tab.verse", "f():void =\n\tA()\n", "structure"),
    ];
    for (file, text, expected) in cases {
        put(root, file, text);
        let out = run(root, &[file, "--check", "--output-format", "json"]);
        assert_eq!(out.status.code(), Some(2), "{file}: {out:?}");
        let report = envelope(&out);
        assert_eq!(
            report["files"][0]["reason"]["stage"], expected,
            "{file}: {report}"
        );
        assert!(
            report["files"][0]["reason"]["offset"].is_number(),
            "{file}: {report}"
        );
        assert_eq!(report["files"][0]["status"], "refused");
        fs::remove_file(root.join(file)).unwrap();
    }
}

#[test]
fn discovery_failures_still_emit_an_envelope_with_null_path() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let out = run(root, &["missing.verse", "--output-format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let report = envelope(&out);
    assert_eq!(report["files"].as_array().unwrap().len(), 0);
    assert_eq!(report["summary"]["complete"], false);
    assert_eq!(
        report["executionErrors"][0]["path"],
        serde_json::Value::Null
    );
    assert!(
        report["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("missing.verse")
    );
}

/// A write failure is an execution error for one path, never a `refused` file:
/// a refusal means the source was analyzed and rejected. Files staged but never
/// committed must also not claim `formatted`, because no byte reached them.
#[cfg(windows)]
#[test]
fn a_write_failure_is_an_execution_error_and_leaves_staged_files_unwritten() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A:=1\n");
    put(root, "z.verse", "Z:=2\n");
    let original = fs::metadata(root.join("z.verse")).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(root.join("z.verse"), readonly).unwrap();

    let out = run(root, &[".", "--write"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("read-only file"),
        "{out:?}"
    );
    // The text path is unchanged and still prints the full host path.
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(&root.display().to_string()),
        "text diagnostic must stay byte-identical: {out:?}"
    );
    assert_eq!(fs::read(root.join("a.verse")).unwrap(), b"A:=1\n");
    assert_eq!(fs::read(root.join("z.verse")).unwrap(), b"Z:=2\n");

    let out = run(root, &[".", "--write", "--output-format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let report = envelope(&out);

    let status = |path: &str| {
        report["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == path)
            .unwrap()["status"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    // Preflight refused the whole batch, so nothing was replaced anywhere.
    assert_eq!(status("a.verse"), "would-format", "{report}");
    assert_eq!(status("z.verse"), "would-format", "{report}");
    assert_eq!(report["summary"]["filesChanged"], 2, "{report}");
    assert_eq!(report["summary"]["filesRefused"], 0, "{report}");
    assert_eq!(report["summary"]["complete"], false, "{report}");
    assert_eq!(
        report["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|file| file["status"] == "refused")
            .count(),
        0,
        "a write failure must never fabricate a refusal: {report}"
    );
    let errors = report["executionErrors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{report}");
    assert_eq!(errors[0]["path"], "z.verse", "{report}");
    let message = errors[0]["message"].as_str().unwrap();
    assert!(message.contains("read-only file"), "{report}");
    assert!(!message.contains('\\'), "{report}");
    assert!(
        !message.contains(&root.display().to_string()),
        "absolute path leaked into executionErrors: {report}"
    );
}

/// Diagnostics built from absolute paths keep their host spelling on stderr and
/// reach the envelope as root-relative labels: the contract forbids absolute
/// paths in machine output.
#[test]
fn machine_messages_carry_no_absolute_paths_while_stderr_keeps_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A := 1\n");
    let absolute = root.join("missing folder").join("gone.verse");
    let absolute = absolute.to_str().unwrap();

    let out = run(root, &[absolute, "--check", "--output-format", "json"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let report = envelope(&out);
    let message = report["executionErrors"][0]["message"].as_str().unwrap();
    assert!(
        !message.contains(root.to_str().unwrap()),
        "absolute path leaked into the envelope: {report}"
    );
    assert!(!message.contains('\\'), "{report}");
    assert!(
        message.contains("missing folder/gone.verse"),
        "the affected source is still identifiable: {report}"
    );

    let text = run(root, &[absolute, "--check"]);
    assert_eq!(text.status.code(), Some(2), "{text:?}");
    assert!(
        String::from_utf8_lossy(&text.stderr).contains(absolute),
        "the text diagnostic must stay byte-identical: {text:?}"
    );
}

/// A successful `--write` still reports `formatted` for exactly the files that
/// were replaced, and `--write` without a failure is unchanged.
#[test]
fn a_successful_write_promotes_only_replaced_files_to_formatted() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A:=1\n");
    put(root, "b.verse", "B := 2\n");
    let out = run(root, &[".", "--write", "--output-format", "json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let report = envelope(&out);
    assert_eq!(report["files"][0]["path"], "a.verse");
    assert_eq!(report["files"][0]["status"], "formatted");
    assert_eq!(report["files"][0]["reason"]["stage"], "layout");
    assert_eq!(report["files"][1]["status"], "unchanged");
    assert_eq!(report["summary"]["filesChanged"], 1);
    assert_eq!(report["summary"]["filesRefused"], 0);
    assert_eq!(report["summary"]["complete"], true);
    assert_eq!(report["executionErrors"].as_array().unwrap().len(), 0);
    assert_eq!(fs::read(root.join("a.verse")).unwrap(), b"A := 1\n");
    assert_eq!(fs::read(root.join("b.verse")).unwrap(), b"B := 2\n");
}
