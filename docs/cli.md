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

## Machine-readable output

`--output-format text|json` selects the stdout format. `text` is the default and
is byte-identical to omitting the flag for stdout, stderr and exit code on the
success path; every formatter diagnostic that names a stage gained the stable
`[stage=…]` marker described below: a refusal, and a discovery, source-read,
write-phase or reporting-envelope failure. Rejecting an invocation happens
before any stage is known, so usage errors (stdin combined with paths, `--write`
with stdin, `json` with `--diff`/`--show-config`/`--explain`, no input, a
directory without a mode), a `--show-config` failure and a failure writing the
ordinary stdout output (check labels, diff, formatted source) keep their
unmarked wording. `json` writes exactly one versioned object on stdout and no
other text;
the envelope, field meanings, path normalization, ordering and stability promise
are in [the report contract](report-contract.md) and
[`diagnostics.schema.json`](diagnostics.schema.json).

```powershell
verse-fmt . --check --output-format json
```

Machine output contains no ANSI escapes, progress text or human banners, no
timestamps and no absolute paths; two runs on the same input produce identical
bytes. Paths are `/`-separated and relative to the config root. Execution errors
still go to stderr **and** appear in the envelope, so a consumer can read
stderr for humans and stdout for machines from one invocation. The machine copy
of a diagnostic has absolute paths relativized to the config root; the stderr
copy keeps the host spelling it has always printed. A write failure is an
`executionErrors` entry only — the file keeps the analysis status it had, so
`would-format` plus a failed write never reads as `formatted` or `refused`. Exit
codes do not change, and `json` never becomes a success signal by itself.
`--write` with `json` is allowed and reports the post-write state. `json` with
`--diff` (the diff is human text), `--show-config` (which prints TOML) or
`--explain` (a human failure chain the envelope already carries) exits 2 with a
message and writes nothing to stdout.

## Stage markers and `--explain`

A refusal reports the stage that stopped processing, in text and in JSON. In the
text diagnostic the marker is the last element of the line and is stable and
greppable:

```text
verse-fmt: device.verse:1:2: unterminated block comment [stage=lexer]
verse-fmt: device.verse:1:35: ambiguous adjacent top-level statements; inline binary function bodies are unsupported by the pinned grammar [stage=parser] [class=guard]
device.verse:2:9: underlying parse error: ERROR node at byte 49; unsupported or incomplete Verse syntax [stage=parser] [class=parse]
```

The `verse-fmt:` prefix belongs to the primary diagnostic only. It is added
once, around the whole stderr message, so a secondary line that follows it starts
with the file label instead of repeating the prefix.

The vocabulary is fixed: `discovery`, `source`, `lexer`, `parser`, `structure`,
`layout`, `validation`, `write`, `reporting`. `[class=…]` appears only where one
stage has two distinguishable outcomes — `guard` for the adjacent-structure
refusal, `parse` for a raw CST `ERROR`/`MISSING` finding, both under `parser`.
The guard is always the primary refusal; a raw parse error underneath it is a
secondary line that follows it, and a source that fails only the guard gets no
second line. A refusal's wording is otherwise unchanged, so existing greps on
`ambiguous adjacent top-level statements` still match.

### `--explain`

`--explain` is text-only. It prints an explanation block per **refused source**:
the stage order the source actually entered and the complete failure chain,
after the ordinary concise diagnostic:

```text
verse-fmt: device.verse:1:35: ambiguous adjacent top-level statements; ... [stage=parser] [class=guard]
device.verse:2:9: underlying parse error: ERROR node at byte 49; ... [stage=parser] [class=parse]
verse-fmt explain: device.verse
  stages: source -> lexer -> parser
  failure 1 (primary, parser/guard, offset 34): 1:35: ambiguous adjacent top-level statements; ...
  failure 2 (secondary, parser/parse, ERROR, offset 49): underlying parse error: ERROR node at byte 49; ...
```

`stages` lists every stage the source entered, in execution order, and always
ends at the stage that stopped processing. It is not a fixed pipeline diagram:
a source that reaches `validation` shows the stages `validation` really enters
to reparse its own output. `--explain` never changes an exit code, never adds
stdout, and prints nothing for successful runs. A failure that is not
traceable to one source — a discovery, read or write failure — still gets its
`[stage=…]` marker but no explanation block, because there is no per-source
stage order to print.

`--explain` with `--output-format json` is rejected with exit 2 and no stdout:
the envelope already carries `reason.stage`, `reason.class` and
`reason.underlying`, so the flag could only duplicate machine data in text.

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
Limit failures are not silent truncation or successful partial formatting. A
discovery limit (visited entries, directory depth, source count, or the
per-directory entry cap recorded in [resource limits](resource-limits.md)) stops
the walk before the next sibling or explicit input. Sources discovered before
it report their own status; later files are neither discovered nor analyzed.
Per-source limits keep the other sources' results. Either way the limit is an
`executionErrors` entry, `complete` is `false`, the exit code is 2, and nothing
is written.
Every one of these has an in-repository test: see
[resource limits and their tests](resource-limits.md), which names the test for
each value and records the limits whose documented behaviour differs from what
this build does.

Prefer file input under Windows PowerShell 5.1: its text pipelines can recode
UTF-8/BOM or line endings before bytes reach the CLI. The tool cannot recover
bytes already changed by a shell.
