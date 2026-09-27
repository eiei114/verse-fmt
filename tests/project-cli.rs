use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn put(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn discovers_sorted_unique_sources_with_explanations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "b.verse", "B:=2\n");
    put(root, "a.VERSE", "A:=1\n");
    put(root, ".hidden.verse", "invalid!");
    put(root, "Generated.digest.verse", "invalid!");
    put(root, "Saved/auto.verse", "invalid!");
    put(root, "Intermediate/auto.verse", "invalid!");
    put(root, "Vendor/auto.verse", "invalid!");
    put(root, "ignored.verse", "invalid!");
    put(root, ".gitignore", "ignored.verse\n");
    put(
        root,
        "verse.toml",
        "[files]\nexclude = ['Vendor/**']\n[lint]\nfuture-rule = true\n",
    );
    let out = run(root, &[".", "b.verse", "--check", "--verbose"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().replace('\r', ""),
        "a.VERSE\nb.verse\n"
    );
    let log = String::from_utf8(out.stderr).unwrap();
    for reason in [
        ".gitignore",
        "hidden",
        "protection",
        "verse.toml exclude",
        "2 eligible",
    ] {
        assert!(log.contains(reason), "{log}");
    }
}

#[test]
fn explicit_files_override_gitignore_but_not_protected_or_config_excludes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, ".gitignore", "a.verse\n");
    put(root, "a.verse", "A:=1\n");
    put(root, ".hidden.verse", "H:=2\n");
    put(root, "Api.digest.verse", "A:=1\n");
    assert_eq!(run(root, &["a.verse", "--check"]).status.code(), Some(1));
    assert_eq!(
        run(root, &[".hidden.verse", "--check"]).status.code(),
        Some(1)
    );
    assert_eq!(
        run(root, &["Api.digest.verse", "--check"]).status.code(),
        Some(2)
    );
    put(root, "verse.toml", "[files]\nexclude=['a.verse']\n");
    assert_eq!(run(root, &["a.verse", "--check"]).status.code(), Some(2));
}

#[test]
fn respects_nested_gitignore_and_negation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, ".gitignore", "*.verse\n!keep.verse\n");
    put(root, "keep.verse", "Keep:=1\n");
    put(root, "other.verse", "Bad!\n");
    put(root, "sub/.gitignore", "!local.verse\n");
    put(root, "sub/local.verse", "Local:=1\n");
    let out = run(root, &[".", "--check"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().replace('\r', ""),
        "keep.verse\nsub/local.verse\n"
    );
}

#[test]
fn config_is_one_per_invocation_and_stops_at_git_boundary() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "verse.toml", "[format]\nline-ending='crlf'\n");
    put(root, "sub/verse.toml", "[format]\nline-ending='lf'\n");
    put(root, "sub/a.verse", "A:=1\n");
    let out = run(root, &["sub/a.verse"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"A := 1\r\n");
    let out = run(&root.join("sub"), &["a.verse"]);
    assert_eq!(out.stdout, b"A := 1\n");
    fs::remove_file(root.join("sub/verse.toml")).unwrap();
    fs::create_dir(root.join("sub/.git")).unwrap();
    assert_eq!(run(&root.join("sub"), &["a.verse"]).stdout, b"A := 1\n");
    assert_eq!(
        run(&root.join("sub"), &["a.verse", "--config", "../verse.toml"]).stdout,
        b"A := 1\r\n"
    );
}

#[test]
fn show_config_validates_own_keys_but_tolerates_sibling_keys() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(
        root,
        "verse.toml",
        "schema-version=1\n[lint]\nfuture='okay'\n",
    );
    let out = run(root, &["--show-config"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let config: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(config["format"]["line-ending"].as_str(), Some("preserve"));
    for text in [
        "schema-version=99",
        "unknown=1",
        "[format]\nunknown=1",
        "[files]\nunknown=1",
        "lint=1",
        "[format]\nline-ending='native'",
        "[files]\nexclude=['!foo']",
    ] {
        put(root, "verse.toml", text);
        let out = run(root, &["--show-config"]);
        assert_eq!(out.status.code(), Some(2), "{text}: {out:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn stdin_virtual_path_selects_configuration_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "sub/verse.toml", "[format]\nline-ending='crlf'\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .args(["-", "--stdin-filepath", "sub/virtual.verse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"A:=1\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"A := 1\r\n");
    assert!(!root.join("sub/virtual.verse").exists());
}

#[test]
fn diff_is_non_mutating_and_color_is_opt_in_for_redirects() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    put(root, "a.verse", "A:=1\n");
    let before = fs::metadata(root.join("a.verse"))
        .unwrap()
        .modified()
        .unwrap();
    let out = run(root, &[".", "--diff"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let diff = String::from_utf8(out.stdout).unwrap();
    assert!(diff.contains("--- a/a.verse"));
    assert!(diff.contains("+A := 1"));
    assert!(!diff.contains('\x1b'));
    assert!(
        run(root, &[".", "--diff", "--color", "always"])
            .stdout
            .contains(&27)
    );
    assert_eq!(fs::read(root.join("a.verse")).unwrap(), b"A:=1\n");
    assert_eq!(
        fs::metadata(root.join("a.verse"))
            .unwrap()
            .modified()
            .unwrap(),
        before
    );
}

#[test]
fn errors_override_differences_and_zero_targets_is_not_success() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    assert_eq!(run(root, &[".", "--check"]).status.code(), Some(2));
    assert_eq!(run(root, &["*.verse", "--check"]).status.code(), Some(2));
    put(root, "a.verse", "A:=1\n");
    put(root, "z.verse", "<# not closed\n");
    let out = run(root, &[".", "--check"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
