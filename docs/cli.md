# CLI and project configuration

Shared toolchain contract version **1**. Compiler/UEFN acceptance is not
established; see [grammar coverage](grammar-coverage.md).

`verse-fmt file.verse` or `verse-fmt -` writes formatted source only to stdout.
Multiple files/directories require exactly one of `--check`, `--diff`, `--write`.
No arguments is an error, not an implicit recursive write. Stdin cannot be a
write target. `--stdin-filepath` is a virtual path for config/labels only.

Exit codes: 0 success/unchanged; 1 check or diff found changes; 2 input, config,
unsupported syntax, invariant, resource or I/O failure. Errors dominate changes.
All inputs are analyzed before source/diff output or writes; formatter failures
do not print partial formatted source. Errors and `--verbose` exclusion reasons
go to stderr. `--color auto|always|never` affects diagnostics/diff, never source.
Auto respects `NO_COLOR` and terminal detection. On Windows, it enables virtual
terminal processing for the selected console stream; if that mode cannot be
enabled, Auto disables color for that stream.

## verse.toml (schema 1)

```toml
schema-version = 1

[files]
exclude = ["Vendor/**", "**/generated/**"]

[format]
line-ending = "preserve" # preserve (default), lf, crlf
```

Search from CWD upwards, including but not crossing the first `.git` boundary.
For virtual stdin, start at the virtual file's parent. `--config path` overrides
search. One config applies to the entire invocation; nested configs never merge.
The root for exclude patterns and labels is the config parent, otherwise CWD.
`--show-config` prints effective settings and source/root comments without source
processing. Unknown top-level, files or format keys fail; the sibling `[lint]`
table is tolerated without validating its rule settings.

Excludes are relative forward-slash globs; no negation, drive prefixes or
backslashes. Local parent/nested `.gitignore` files and negation are supported;
user-global Git ignores are intentionally not loaded. Hidden entries are skipped
recursively. An explicit file bypasses hidden/gitignore filtering, but never
config excludes, `.git`, `Intermediate`, `Saved` or `*.digest.verse` protection.
Recursive links/junctions/reparse points are skipped; explicit ones (including
linked ancestors) are errors. Files are identity-deduplicated and sorted by
relative forward-slash labels. Missing paths, shell-unexpanded globs and zero
eligible files fail with exit 2.

Limits: 8 MiB/source, 64 MiB/total input, 10,000 sources, 100,000 visited entries,
128 directory levels; 256 KiB/config or ignore file; 256 exclude globs, each at
most 4,096 bytes. Parser/token/depth limits are in [parser design](parser-design.md).
Limit failures are not silent truncation or successful partial formatting.

Prefer file input under Windows PowerShell 5.1: its text pipelines can recode
UTF-8/BOM or line endings before bytes reach the CLI. The tool cannot recover
bytes already changed by a shell.
