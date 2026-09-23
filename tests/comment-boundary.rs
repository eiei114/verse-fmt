//! Original ambiguous comment example, not copied from a private project.
use std::{fs, process::Command};

const MISMATCH: &str = "F():void =\n    if:\n        A := First[] <# keep\n        comment #> B := Second[]\n    then:\n        Print(\"yes\")\n";

#[test]
fn mismatched_block_comment_fails_before_check_or_write() {
    let source = MISMATCH;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("comment.verse");
    fs::write(&path, source).unwrap();
    for mode in ["--check", "--write"] {
        let out = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
            .current_dir(dir.path())
            .arg(mode)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{mode}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("block comment boundary"));
        assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
    }
}

#[test]
fn matched_comments_and_opaque_literals_keep_bytes_with_bom_and_crlf() {
    for prefix in [
        "<# outer <# inner #> end #>\n",
        "<# one #><# two #>\n",
        "<# multi\nline #>\n",
        "Label := \"keep <# text #>\"\n",
        "Count := 1\nLabel := \"keep <# text #> {Count}\"\n",
        "# <# not a block #>\n",
    ] {
        for bom in ["", "\u{feff}"] {
            for eol in ["\n", "\r\n"] {
                let source = format!("{bom}{prefix}Value:=1  \n").replace('\n', eol);
                let expected = format!("{bom}{prefix}Value := 1\n").replace('\n', eol);
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("positive.verse");
                fs::write(&path, &source).unwrap();
                for _ in 0..2 {
                    let out = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
                        .current_dir(dir.path())
                        .arg("--write")
                        .arg(&path)
                        .output()
                        .unwrap();
                    assert_eq!(out.status.code(), Some(0), "{source:?}: {out:?}");
                    assert_eq!(fs::read(&path).unwrap(), expected.as_bytes());
                }
            }
        }
    }
}

#[test]
fn earliest_mismatch_position_is_deterministic_with_bom_and_crlf() {
    let source =
        format!("\u{feff}{MISMATCH}{}", MISMATCH.replace("F()", "G()")).replace('\n', "\r\n");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multiple.verse");
    fs::write(&path, &source).unwrap();
    let mut previous = None;
    for _ in 0..8 {
        let out = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
            .current_dir(dir.path())
            .arg("--check")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr)
                .contains(":3:22: unsupported block comment boundary"),
            "{out:?}"
        );
        if let Some(ref bytes) = previous {
            assert_eq!(&out.stderr, bytes);
        }
        previous = Some(out.stderr);
    }
    assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
}

#[test]
fn later_mismatch_prevents_earlier_file_write() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("a.verse");
    let later = dir.path().join("z.verse");
    fs::write(&first, "Value:=1  \n").unwrap();
    fs::write(&later, MISMATCH).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(dir.path())
        .arg("--write")
        .arg(&first)
        .arg(&later)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("block comment boundary"));
    assert_eq!(fs::read(&first).unwrap(), b"Value:=1  \n");
    assert_eq!(fs::read(&later).unwrap(), MISMATCH.as_bytes());
}
