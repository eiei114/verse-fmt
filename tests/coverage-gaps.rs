//! Self-authored practical shapes, not copied project code or compiler evidence.
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn practical_syntax_has_golden_output_and_is_idempotent() {
    for (source, expected) in [
        ("using { Demo.Helpers }\n", "using { Demo.Helpers }\n"),
        (
            "using { Demo.Helpers.More }\n",
            "using { Demo.Helpers.More }\n",
        ),
        ("using { Helpers }\n", "using { Helpers }\n"),
        (
            "shade := enum{\n    Light,\n    Dark\n}\n",
            "shade := enum{\n    Light,\n    Dark\n}\n",
        ),
        (
            "shade := enum{Light,Dark}\n",
            "shade := enum{Light, Dark}\n",
        ),
        (
            "F():void =\n  Label:string=\"hello\"\n",
            "F():void =\n    Label:string = \"hello\"\n",
        ),
        (
            "F():void =\n    Count := 1\n    Label:string=\"{Count}\"\n",
            "F():void =\n    Count := 1\n    Label:string = \"{Count}\"\n",
        ),
        ("Count:int=1\n", "Count:int = 1\n"),
    ] {
        for source in [source, expected] {
            let dir = tempfile::tempdir().unwrap();
            let mut child = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
                .current_dir(dir.path())
                .arg("-")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(source.as_bytes())
                .unwrap();
            let out = child.wait_with_output().unwrap();
            assert_eq!(out.status.code(), Some(0), "{source}: {out:?}");
            assert_eq!(out.stdout, expected.as_bytes(), "{source}");
            assert!(out.stderr.is_empty());
        }
    }
}
