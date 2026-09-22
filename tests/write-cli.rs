use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-fmt"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn writes_and_then_leaves_mtime_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("日本語 device.verse");
    fs::write(&path, b"\xef\xbb\xbfA:=1  \r\n").unwrap();
    let out = run(root.path(), &[".", "--write"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty());
    assert_eq!(fs::read(&path).unwrap(), b"\xef\xbb\xbfA := 1\r\n");
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(run(root.path(), &[".", "--write"]).status.code(), Some(0));
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn malformed_file_prevents_all_writes() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.verse"), b"A:=1\n").unwrap();
    fs::write(root.path().join("z.verse"), b"<# not closed\n").unwrap();
    assert_eq!(run(root.path(), &[".", "--write"]).status.code(), Some(2));
    assert_eq!(fs::read(root.path().join("a.verse")).unwrap(), b"A:=1\n");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[test]
fn hard_links_are_never_replaced() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a.verse");
    let link = root.path().join("b.verse");
    fs::write(&path, b"A:=1\n").unwrap();
    fs::hard_link(&path, &link).unwrap();
    assert_eq!(
        run(root.path(), &["a.verse", "--write"]).status.code(),
        Some(2)
    );
    assert_eq!(fs::read(&path).unwrap(), b"A:=1\n");
    assert_eq!(fs::read(&link).unwrap(), b"A:=1\n");
}

#[cfg(windows)]
#[test]
fn readonly_preflight_keeps_every_file_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a.verse");
    let z = root.path().join("z.verse");
    fs::write(&a, b"A:=1\n").unwrap();
    fs::write(&z, b"Z:=2\n").unwrap();
    let original = fs::metadata(&z).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&z, readonly).unwrap();
    let out = run(root.path(), &[".", "--write"]);
    fs::set_permissions(&z, original).unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(fs::read(&a).unwrap(), b"A:=1\n");
    assert_eq!(fs::read(&z).unwrap(), b"Z:=2\n");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}

#[cfg(windows)]
#[test]
fn a_windows_file_lock_fails_without_truncation() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a.verse");
    fs::write(&path, b"A:=1\n").unwrap();
    let guard = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let out = run(root.path(), &["a.verse", "--write"]);
    drop(guard);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(fs::read(&path).unwrap(), b"A:=1\n");
}

#[cfg(windows)]
#[test]
fn junctions_and_paths_through_them_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let actual = root.path().join("actual");
    let junction = root.path().join("junction");
    fs::create_dir(&actual).unwrap();
    fs::write(actual.join("a.verse"), b"A:=1\n").unwrap();
    let made = Command::new("cmd.exe")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&actual)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let out = run(root.path(), &["junction/a.verse", "--write"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(fs::read(actual.join("a.verse")).unwrap(), b"A:=1\n");
    fs::remove_dir(&junction).unwrap();
}

#[cfg(windows)]
#[test]
fn replacement_preserves_creation_time_custom_dacl_and_named_stream() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("security.verse");
    fs::write(&path, b"A:=1\n").unwrap();
    let stream = format!("{}:verse-test", path.display());
    fs::write(&stream, b"preserve this stream").unwrap();
    let get_sddl =
        "$ErrorActionPreference='Stop'; (Get-Acl -LiteralPath $env:VERSE_TEST_FILE).Sddl";
    let set_sddl = "$ErrorActionPreference='Stop'; $acl=Get-Acl -LiteralPath $env:VERSE_TEST_FILE; $acl.SetAccessRuleProtection($true,$true); Set-Acl -LiteralPath $env:VERSE_TEST_FILE -AclObject $acl; (Get-Acl -LiteralPath $env:VERSE_TEST_FILE).Sddl";
    let powershell = |script: &str| {
        let out = Command::new("pwsh")
            .args(["-NoProfile", "-Command", script])
            .env("VERSE_TEST_FILE", &path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let before_acl = powershell(set_sddl);
    let before_created = fs::metadata(&path).unwrap().created().unwrap();
    let out = run(root.path(), &["security.verse", "--write"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(powershell(get_sddl), before_acl);
    assert_eq!(
        fs::metadata(&path).unwrap().created().unwrap(),
        before_created
    );
    assert_eq!(fs::read(stream).unwrap(), b"preserve this stream");
    assert_eq!(fs::read(&path).unwrap(), b"A := 1\n");
}
