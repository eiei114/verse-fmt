//! Guarded Windows replacement. Backups are required: ReplaceFileW is not a
//! blanket guarantee that the original pathname survives every failure.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use crate::{files, source::MAX_SOURCE_BYTES};

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    modified: SystemTime,
    created: Option<SystemTime>,
    len: u64,
    readonly: bool,
    identity: (u64, u64),
    links: u64,
    attributes: u32,
}

fn stamp(file: &File) -> io::Result<Stamp> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    let (identity, links, attributes) = {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        // SAFETY: zero is valid for all integer fields; the OS fills the struct
        // only on success. The borrowed raw handle remains owned by File.
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        (
            (
                u64::from(info.dwVolumeSerialNumber),
                (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            ),
            u64::from(info.nNumberOfLinks),
            info.dwFileAttributes,
        )
    };
    #[cfg(unix)]
    let (identity, links, attributes) = {
        use std::os::unix::fs::MetadataExt;
        (
            (metadata.dev(), metadata.ino()),
            metadata.nlink(),
            metadata.mode(),
        )
    };
    #[cfg(not(any(windows, unix)))]
    let (identity, links, attributes) = ((0, 0), 0, 0);
    Ok(Stamp {
        modified: metadata.modified()?,
        created: metadata.created().ok(),
        len: metadata.len(),
        readonly: metadata.permissions().readonly(),
        identity,
        links,
        attributes,
    })
}

fn read_bytes(file: &mut File) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(io::Error::other("source exceeds the 8 MiB limit"));
    }
    Ok(bytes)
}

pub struct Snapshot {
    pub path: Option<PathBuf>,
    pub bytes: Vec<u8>,
    stamp: Option<Stamp>,
}

impl Snapshot {
    pub fn read(path: &Path) -> Result<Self, String> {
        files::guard_path(path)?;
        let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let before = stamp(&file).map_err(|e| e.to_string())?;
        let bytes = read_bytes(&mut file).map_err(|e| e.to_string())?;
        if before != stamp(&file).map_err(|e| e.to_string())? {
            return Err("file changed while being read".into());
        }
        Ok(Self {
            path: Some(path.into()),
            bytes,
            stamp: Some(before),
        })
    }

    pub fn stdin(bytes: Vec<u8>) -> Self {
        Self {
            path: None,
            bytes,
            stamp: None,
        }
    }
}

pub struct Pending {
    snapshot: Snapshot,
    guard: File,
    replacement: tempfile::TempPath,
    backup: tempfile::TempDir,
}

fn open_guard(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Deny concurrent writers while permitting our rename/replacement.
        options.share_mode(1 | 4); // FILE_SHARE_READ | FILE_SHARE_DELETE
        // Request DELETE now, so an existing read-only sharing handle that
        // denies replacement fails during all-file preflight, not mid-batch.
        options.access_mode(0x8000_0000 | 0x0001_0000); // GENERIC_READ | DELETE
    }
    options.open(path)
}

pub fn prepare(snapshot: Snapshot, output: &[u8]) -> Result<Pending, String> {
    prepare_using(snapshot, output, |file, bytes| file.write_all(bytes))
}

fn prepare_using(
    snapshot: Snapshot,
    output: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> Result<Pending, String> {
    let path = snapshot
        .path
        .as_deref()
        .ok_or("stdin is not a write target")?;
    files::guard_path(path)?;
    let mut guard = open_guard(path)
        .map_err(|e| format!("{}: cannot lock for replacement: {e}", path.display()))?;
    let current = stamp(&guard).map_err(|e| e.to_string())?;
    if current.readonly {
        return Err(format!("{}: read-only file", path.display()));
    }
    #[cfg(windows)]
    if current.attributes & !(2 | 4 | 32 | 128 | 8192) != 0 {
        return Err(format!(
            "{}: unsupported Windows file attributes; compressed, encrypted, sparse or cloud-managed files are not write targets",
            path.display()
        ));
    }
    if current.links != 1 {
        return Err(format!(
            "{}: hard-linked or unsupported file identity",
            path.display()
        ));
    }
    if Some(&current) != snapshot.stamp.as_ref()
        || read_bytes(&mut guard).map_err(|e| e.to_string())? != snapshot.bytes
    {
        return Err(format!("{}: file changed after analysis", path.display()));
    }
    let parent = path.parent().ok_or("write target has no parent")?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".verse-write-")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    verify_owner_group(&guard, temporary.path()).map_err(|e| e.to_string())?;
    copy_dacl(temporary.path(), &guard)
        .map_err(|e| format!("cannot protect staged source: {e}"))?;
    write(temporary.as_file_mut(), output).map_err(|e| format!("cannot stage replacement: {e}"))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| format!("cannot sync replacement: {e}"))?;
    copy_attributes(temporary.path(), &guard, current.attributes)
        .map_err(|e| format!("cannot preserve file attributes: {e}"))?;
    let replacement = temporary.into_temp_path(); // Close handle before ReplaceFileW's exclusive open.
    let backup = tempfile::Builder::new()
        .prefix(".verse-backup-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    copy_dacl(backup.path(), &guard)
        .map_err(|e| format!("cannot protect backup directory: {e}"))?;
    // Keep an independently synced byte copy before the OS call as well as the
    // metadata-preserving OS backup, even for unusual partial failure codes.
    let mut recovery = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(backup.path().join("recovery.verse"))
        .map_err(|e| e.to_string())?;
    copy_dacl(&backup.path().join("recovery.verse"), &guard)
        .map_err(|e| format!("cannot protect recovery source: {e}"))?;
    recovery
        .write_all(&snapshot.bytes)
        .and_then(|()| recovery.sync_all())
        .map_err(|e| format!("cannot stage recovery copy: {e}"))?;
    drop(recovery);
    Ok(Pending {
        snapshot,
        guard,
        replacement,
        backup,
    })
}

impl Pending {
    pub fn commit(self) -> Result<Option<String>, String> {
        self.commit_using(native_replace)
    }

    fn commit_using(
        self,
        replace: impl FnOnce(&Path, &Path, &Path) -> io::Result<()>,
    ) -> Result<Option<String>, String> {
        let Self {
            snapshot,
            guard,
            replacement,
            backup,
        } = self;
        let path = snapshot.path.as_deref().ok_or("missing write path")?;
        files::guard_path(path)?;
        // Reopen by name to detect path swaps, not just edits to our old handle.
        let mut current = File::open(path).map_err(|e| e.to_string())?;
        if Some(&stamp(&current).map_err(|e| e.to_string())?) != snapshot.stamp.as_ref()
            || read_bytes(&mut current).map_err(|e| e.to_string())? != snapshot.bytes
        {
            return Err(format!(
                "{}: file changed before replacement",
                path.display()
            ));
        }
        verify_owner_group(&current, &replacement).map_err(|e| e.to_string())?;
        drop(current);
        // Disarm automatic cleanup before any call that can rename the original.
        let backup_dir = backup.keep();
        let backup_file = backup_dir.join("original.verse");
        let result = replace(path, &replacement, &backup_file);
        drop(guard);
        match result {
            Ok(()) => {
                // ReplaceFileW moves whichever file held `path` at call time.
                // Keep both recovery copies if a concurrent rename won the gap
                // between our final snapshot check and the atomic replacement.
                let replaced_original = Snapshot::read(&backup_file).is_ok_and(|saved| {
                    saved.bytes == snapshot.bytes && saved.stamp == snapshot.stamp
                });
                if !replaced_original {
                    return Err(format!(
                        "{}: destination changed during replacement; replaced destination and recovery copy retained at {}",
                        path.display(),
                        backup_dir.display()
                    ));
                }
                if let Err(e) = clean_backup(&backup_dir) {
                    return Ok(Some(format!(
                        "saved {}; backup cleanup failed ({e}); recovery retained at {}",
                        path.display(),
                        backup_dir.display()
                    )));
                }
                Ok(None)
            }
            Err(error) => {
                // Restore only an absent destination, never overwrite a user edit.
                if fs::symlink_metadata(path).is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
                    && let Ok(original) = Snapshot::read(&backup_file)
                    && original.bytes == snapshot.bytes
                    && original.stamp == snapshot.stamp
                {
                    let _ = native_restore(&backup_file, path);
                }
                let unchanged = Snapshot::read(path)
                    .is_ok_and(|now| now.bytes == snapshot.bytes && now.stamp == snapshot.stamp);
                if unchanged && clean_backup(&backup_dir).is_ok() {
                    Err(format!("replacement failed; original preserved: {error}"))
                } else {
                    // Keep recovery bytes AND any metadata-preserving OS backup.
                    // Do not roll back already completed files in the batch.
                    let retained = replacement.keep().ok();
                    Err(format!(
                        "replacement failed ({error}); inspect {} and recovery directory {}; staged replacement {:?}. No existing destination was overwritten during recovery",
                        path.display(),
                        backup_dir.display(),
                        retained
                    ))
                }
            }
        }
    }
}

fn clean_backup(directory: &Path) -> io::Result<()> {
    for name in ["original.verse", "recovery.verse"] {
        match fs::remove_file(directory.join(name)) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    fs::remove_dir(directory)
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn native_replace(path: &Path, temporary: &Path, backup: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    let (path, temporary, backup) = (wide(path), wide(temporary), wide(backup));
    // SAFETY: all strings are owned, NUL-terminated OS paths alive for the call.
    // Flags 0: never ignore ACL/attribute merging errors. Always request backup.
    if unsafe {
        ReplaceFileW(
            path.as_ptr(),
            temporary.as_ptr(),
            backup.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn native_restore(backup: &Path, path: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let (backup, path) = (wide(backup), wide(path));
    // SAFETY: borrowed pointers are valid; flags 0 refuses an existing target.
    if unsafe { MoveFileExW(backup.as_ptr(), path.as_ptr(), 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn copy_attributes(path: &Path, _original: &File, attributes: u32) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::SetFileAttributesW;
    let path = wide(path);
    let basic = attributes & (2 | 4 | 32 | 8192); // hidden, system, archive, not-content-indexed
    // SAFETY: valid owned path; other attributes and ACLs are merged by ReplaceFileW.
    if unsafe { SetFileAttributesW(path.as_ptr(), if basic == 0 { 128 } else { basic }) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn copy_attributes(path: &Path, original: &File, _attributes: u32) -> io::Result<()> {
    fs::set_permissions(path, original.metadata()?.permissions())
}

#[cfg(not(windows))]
fn native_replace(_path: &Path, _temporary: &Path, _backup: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "safe replacement is currently verified only on Windows",
    ))
}

#[cfg(not(windows))]
fn native_restore(_backup: &Path, _path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows recovery only",
    ))
}

#[cfg(windows)]
fn verify_owner_group(original: &File, replacement: &Path) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetNamedSecurityInfoW, GetSecurityInfo, SE_FILE_OBJECT},
            EqualSid, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSID,
        },
    };
    let (mut owner, mut group, mut descriptor) = (
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let flags = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION;
    // SAFETY: live borrowed handle; out pointers own descriptor storage, freed below.
    let error = unsafe {
        GetSecurityInfo(
            original.as_raw_handle(),
            SE_FILE_OBJECT,
            flags,
            &mut owner,
            &mut group,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let (mut other_owner, mut other_group, mut other_descriptor) = (
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    let path = wide(replacement);
    // SAFETY: live NUL-terminated path, writable out pointers.
    let error = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            flags,
            &mut other_owner,
            &mut other_group,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut other_descriptor,
        )
    };
    let result = if error != 0 {
        Err(io::Error::from_raw_os_error(error as i32))
    } else {
        let same = |a: PSID, b: PSID| {
            if a.is_null() || b.is_null() {
                a == b
            }
            // SAFETY: non-null SIDs belong to the still-live security descriptors.
            else {
                unsafe { EqualSid(a, b) != 0 }
            }
        };
        if same(owner, other_owner) && same(group, other_group) {
            Ok(())
        } else {
            Err(io::Error::other(
                "cannot preserve original owner/group; refusing replacement without changing ownership",
            ))
        }
    };
    // SAFETY: both allocations come from the security APIs; null LocalFree is safe.
    unsafe {
        LocalFree(descriptor);
        LocalFree(other_descriptor);
    }
    result
}

#[cfg(not(windows))]
fn verify_owner_group(_original: &File, _replacement: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(windows)]
fn copy_dacl(path: &Path, original: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{GetSecurityInfo, SE_FILE_OBJECT, SetNamedSecurityInfoW},
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl,
            PROTECTED_DACL_SECURITY_INFORMATION, UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: valid borrowed file handle and writable out-pointers. The returned
    // descriptor owns dacl's storage and is released with LocalFree below.
    let error = unsafe {
        GetSecurityInfo(
            original.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32));
    }
    let result = (|| {
        let (mut control, mut revision) = (0, 0);
        // SAFETY: descriptor remains allocated through this entire closure.
        if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let protection = if control & 0x1000 != 0 {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        let mut name = wide(path);
        // Apply restrictions while scratch files are still empty, before they
        // hold source content. These are owned random paths in a trusted folder.
        let error = unsafe {
            SetNamedSecurityInfoW(
                name.as_mut_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | protection,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null(),
            )
        };
        if error != 0 {
            Err(io::Error::from_raw_os_error(error as i32))
        } else {
            Ok(())
        }
    })();
    // SAFETY: descriptor was allocated by GetSecurityInfo; no references escape.
    unsafe {
        LocalFree(descriptor);
    }
    result
}

#[cfg(not(windows))]
fn copy_dacl(_path: &Path, _original: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn concurrent_change_after_analysis_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.verse");
        fs::write(&path, b"A:=1\n").unwrap();
        let snapshot = Snapshot::read(&path).unwrap();
        fs::write(&path, b"A:=99\n").unwrap();
        assert!(prepare(snapshot, b"A := 1\n").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"A:=99\n");
    }

    #[test]
    fn held_guard_denies_writes_and_detects_namespace_replacement() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.verse");
        let moved = root.path().join("moved.verse");
        fs::write(&path, b"A:=1\n").unwrap();
        let pending = prepare(Snapshot::read(&path).unwrap(), b"A := 1\n").unwrap();
        assert!(fs::write(&path, b"concurrent write").is_err());
        fs::rename(&path, &moved).unwrap();
        fs::write(&path, b"A:=1\n").unwrap();
        assert!(pending.commit().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"A:=1\n");
        assert_eq!(fs::read(&moved).unwrap(), b"A:=1\n");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn staging_failure_preserves_original_and_removes_temporary_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.verse");
        fs::write(&path, b"A:=1\n").unwrap();
        let result = prepare_using(Snapshot::read(&path).unwrap(), b"A := 1\n", |file, _| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected disk-full error"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"A:=1\n");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn replacement_failure_after_backup_restores_original_without_overwrite() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.verse");
        fs::write(&path, b"A:=1\n").unwrap();
        let pending = prepare(Snapshot::read(&path).unwrap(), b"A := 1\n").unwrap();
        let result = pending.commit_using(|path, _, backup| {
            fs::rename(path, backup)?;
            Err(io::Error::other("injected partial replacement failure"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"A:=1\n");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn recovery_never_overwrites_a_new_destination() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.verse");
        fs::write(&path, b"A:=1\n").unwrap();
        let pending = prepare(Snapshot::read(&path).unwrap(), b"A := 1\n").unwrap();
        let result = pending.commit_using(|path, _, backup| {
            fs::rename(path, backup)?;
            fs::write(path, b"user edit")?;
            Err(io::Error::other("injected replacement race"))
        });
        let message = result.unwrap_err();
        assert!(message.contains("recovery directory"), "{message}");
        assert_eq!(fs::read(&path).unwrap(), b"user edit");
        let backup = fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.is_dir())
            .unwrap();
        assert_eq!(fs::read(backup.join("recovery.verse")).unwrap(), b"A:=1\n");
    }

    #[test]
    fn successful_replace_retains_unexpected_concurrent_destination() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.verse");
        let displaced = root.path().join("displaced.verse");
        fs::write(&path, b"A:=1\n").unwrap();
        let pending = prepare(Snapshot::read(&path).unwrap(), b"A := 1\n").unwrap();
        let result = pending.commit_using(|path, replacement, backup| {
            fs::rename(path, &displaced)?;
            fs::write(path, b"concurrent user edit")?;
            fs::rename(path, backup)?;
            fs::rename(replacement, path)?;
            Ok(())
        });
        let message = result.unwrap_err();
        assert!(
            message.contains("replaced destination and recovery copy retained"),
            "{message}"
        );
        assert_eq!(fs::read(&path).unwrap(), b"A := 1\n");
        assert_eq!(fs::read(&displaced).unwrap(), b"A:=1\n");
        let backup = fs::read_dir(root.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|candidate| candidate.is_dir())
            .unwrap();
        assert_eq!(
            fs::read(backup.join("original.verse")).unwrap(),
            b"concurrent user edit"
        );
        assert_eq!(fs::read(backup.join("recovery.verse")).unwrap(), b"A:=1\n");
    }
}
