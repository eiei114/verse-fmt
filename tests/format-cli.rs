use std::io::Write;
use std::process::{Command, Output, Stdio};

fn stdin(input: &[u8], flags: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .arg("-")
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn formats_a_constant_from_stdin() {
    let out = stdin(b"Count:=1  ", &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, b"Count := 1\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn preserves_bom_and_crlf() {
    let out = stdin(b"\xef\xbb\xbfCount:=1  \r\n", &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, b"\xef\xbb\xbfCount := 1\r\n");
}

#[test]
fn check_returns_one_then_zero_without_source_output() {
    let out = stdin(b"Count:=1\n", &["--check"]);
    assert_eq!(out.status.code(), Some(1), "{:?}", out);
    assert_eq!(out.stdout, b"<stdin>\n");
    let out = stdin(b"Count := 1\n", &["--check"]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert!(out.stdout.is_empty());
}

#[test]
fn protects_strings_comments_and_interpolation() {
    let source =
        "<# outer <# inner #> stays  #>\nLabel:=\"日本語 {Format(\"quoted\")}  \" # Keep  \n";
    let expected =
        "<# outer <# inner #> stays  #>\nLabel := \"日本語 {Format(\"quoted\")}  \" # Keep  \n";
    let out = stdin(source.as_bytes(), &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(String::from_utf8(out.stdout).unwrap(), expected);
}

#[test]
fn formats_a_minimal_device_without_moving_blocks() {
    let out = stdin(include_bytes!("fixtures/device.input.verse"), &[]);
    assert_eq!(out.status.code(), Some(0), "{:?}", out);
    assert_eq!(out.stdout, include_bytes!("fixtures/device.expected.verse"));
}

#[test]
fn rejects_incomplete_or_unsupported_input_without_partial_output() {
    for input in [
        &b"<# unterminated"[..],
        b"Label := \"unterminated",
        b"<#> unsupported indented comment\n    text\nCount:=1\n",
        b"Count := (1\n",
        b"A:=1\r\nB:=2\n",
        b"\xff",
        b"A:=1\0\n",
        b"Text:=<p>Hello</p>\n",
    ] {
        let out = stdin(input, &[]);
        assert_eq!(out.status.code(), Some(2), "{input:?}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(!out.stderr.is_empty());
    }
}

#[test]
fn empty_input_stays_empty_and_comment_eof_is_safe() {
    for (input, expected) in [(&b""[..], &b""[..]), (&b"# Keep  "[..], &b"# Keep  \n"[..])] {
        let out = stdin(input, &[]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(out.stdout, expected);
    }
}

#[test]
fn file_with_unicode_and_spaces_is_read_without_mutation() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("日本語 device.verse");
    std::fs::write(&file, b"Count:=1  \r\n").unwrap();
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(out.stdout, b"Count := 1\r\n");
    assert_eq!(std::fs::read(&file).unwrap(), b"Count:=1  \r\n");
    assert_eq!(
        std::fs::metadata(&file).unwrap().modified().unwrap(),
        before
    );
}

#[test]
fn help_bypasses_missing_configuration() {
    let out = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .args(["--config", "missing-configuration.toml", "--help"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8(out.stdout).unwrap().contains("--check"));
}
