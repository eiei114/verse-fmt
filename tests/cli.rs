use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .args(args)
        .output()
        .expect("launch verse-fmt")
}

#[test]
fn help_succeeds_without_reading_input() {
    let out = cli(&["--help"]);
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for flag in ["--write", "--check", "--diff", "--stdin-filepath"] {
        assert!(help.contains(flag), "missing {flag}");
    }
}

#[test]
fn version_is_the_package_version() {
    let out = cli(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        concat!("verse-fmt ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn invalid_flags_fail_without_source_output() {
    for args in [
        vec!["--unknown"],
        vec![".", "--write", "--check"],
        vec![".", "--check", "--diff"],
        vec!["-", "--write"],
        vec![],
    ] {
        let out = cli(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(!out.stderr.is_empty());
    }
}

#[test]
fn nonexistent_file_is_not_success() {
    let out = cli(&["does-not-exist.verse"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
