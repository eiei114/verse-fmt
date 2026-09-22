# Guarded Windows writes

`--write` is currently implemented and tested for Windows local files. This is
not a multi-file transaction or a claim of safety against a hostile filesystem.
Other platforms have no supported write adapter yet; read-only paths are not
deliberately disabled. UEFN and clean-machine verification remain separate.

1. Snapshot bytes, file identity, link count, timestamps and attributes. Reject
   unsupported/reparse paths, mixed encodings/endings and unsafe syntax.
2. Analyze all candidates and validate lexical/CST/indentation equivalence and
   idempotence. Do not write unchanged files, preserving their modification time.
3. Preflight every changed file before the first replacement. Refuse read-only
   files and hardlinks; hold read handles denying writes but permitting delete.
   Recheck original stamp and bytes. Stage same-directory output and a synced
   original-byte recovery copy. Apply source DACL/protection to scratch files
   before storing source bytes there; never alter the source ACL during staging.
4. Revalidate path and reopened identity/bytes, then use `ReplaceFileW` with a
   backup path and no ignore-ACL flags. There is no delete-before-rename fallback.
5. On a failed replacement, restore a matching metadata-preserving backup only
   if the destination is absent. Never overwrite a newly created destination.
   Preserve recovery paths when restoration cannot safely be proven. A cleanup
   failure after successful replacement warns but retains recoverable files.

Backups are required because documented ReplaceFileW failures 1176/1177 can
rename or remove the original name; a simple "atomic rename" claim is wrong.
See [Microsoft ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew).
Scratch recovery directories use `.verse-backup-*`; candidate files use
`.verse-write-*`. Failed operations report retained paths. Inspect them before
manual recovery; do not blindly replace a newer user file.

Validated cases include BOM/CRLF/Unicode paths, unchanged mtime, creation time,
protected custom DACL, NTFS named stream, read-only files, Windows share locks,
hardlinks, junctions, edits after analysis, namespace replacement, injected
staging failure, backup restoration and a competing destination during failure.
Unusual file ownership/group and additional filesystem behavior still require
audit; DACL/stream tests do not establish all possible metadata preservation.

Preflight failure leaves all source files unchanged. A later commit failure can
leave earlier files successfully formatted; the CLI reports completed and
not-attempted paths with exit 2. It does not roll back earlier successes and
risk overwriting subsequent user edits. Parent-path races cannot be completely
eliminated by these checks; use trusted developer directories. Network filesystems,
power-loss durability and adversarial filesystem mutation are not verified.
