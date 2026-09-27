#![cfg(windows)]
use std::{
    fs,
    io::Read,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    process::{Command, Stdio},
};
fn run(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn basic_attributes_survive_and_unusual_group_is_refused_without_ownership_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.verse");
    fs::write(&path, b"A:=1\n").unwrap();
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let flags = 2 | 4 | 32 | 8192;
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(wide.as_ptr(), flags)
        },
        0
    );
    assert!(run(dir.path(), &["a.verse", "--write"]).status.success());
    assert_eq!(
        fs::metadata(&path).unwrap().file_attributes() & flags,
        flags
    );
    fs::write(&path, b"A:=2\n").unwrap();
    let changed=Command::new("pwsh").args(["-NoProfile","-Command","$ErrorActionPreference='Stop'; $a=Get-Acl -LiteralPath $env:VERSE_FILE; $a.SetGroup([Security.Principal.SecurityIdentifier]::new('S-1-5-32-545')); Set-Acl -LiteralPath $env:VERSE_FILE -AclObject $a"])
        .env("VERSE_FILE",&path).output().unwrap();
    assert!(changed.status.success(), "{changed:?}");
    let out = run(dir.path(), &["a.verse", "--write"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("owner/group")
    );
    assert_eq!(fs::read(&path).unwrap(), b"A:=2\n");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[test]
fn long_unicode_paths_work_without_truncation_and_oversized_source_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut folder = dir.path().to_path_buf();
    for i in 0..7 {
        folder = folder.join(format!("long-directory-{i}-abcdefghijklmnop"));
    }
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("日本語 😀.verse");
    fs::write(&path, b"A:=1\n").unwrap();
    assert!(path.as_os_str().len() > 260);
    let out = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .arg(&path)
        .arg("--write")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(fs::read(&path).unwrap(), b"A := 1\n");
    let huge = dir.path().join("large.verse");
    fs::write(&huge, vec![b' '; 8 * 1024 * 1024 + 1]).unwrap();
    let out = run(dir.path(), &["large.verse", "--write"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(fs::metadata(huge).unwrap().len(), 8 * 1024 * 1024 + 1);
}
#[test]
fn closed_stdout_is_execution_failure_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.verse"),
        format!("A:=\"{}\"\n", "x".repeat(1024 * 1024)),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(dir.path())
        .arg("a.verse")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let mut error = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(2), "{error}");
    assert!(!error.contains("panicked"));
}
