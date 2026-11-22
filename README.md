# verse-fmt

Conservative formatter for Epic Games' Verse language and UEFN projects.

> Unreleased alpha under active implementation. File/stdin/project formatting,
> configuration, check, diff and guarded Windows writes work for a bounded
> syntax subset. The self-authored corpus and local performance baseline pass,
> but UEFN compiler acceptance, broad-project readiness and distribution checks
> are still pending. Unsupported syntax fails with exit code 2.
>
> Corpus counts: 151 cases total, 121 golden and 30 refusal. A golden case is a
> successful format; a refusal is an explicit exit 2, not a success. Neither
> count is UEFN compiler evidence, and `tests/docs-drift.rs` fails when a
> document quotes any other number. See [Known limitations](#known-limitations)
> and the [Release gate](#release-gate).

## Usage

```powershell
cargo run -- path/to/device.verse          # formatted UTF-8 to stdout
cargo run -- path/to/device.verse --check  # exit 0 clean, 1 needs formatting, 2 failure
cargo run -- . --diff                     # preview project changes
cargo run -- . --write                    # modify eligible sources after preflight
cargo run -- --show-config                # effective verse.toml settings
cargo run -- . --check --output-format json  # one versioned JSON object on stdout
cargo run -- . --check --explain         # executed stages and full failure chain
```

`--output-format text|json` selects the stdout format; `text` is the default.
The `json` envelope, its schema and the stability promise are in the
[report contract](docs/report-contract.md) and
[`diagnostics.schema.json`](docs/diagnostics.schema.json), so CI can read
formatter results as structured data and join on a `/`-separated path relative
to the config root. `json` never changes an exit code and never becomes a
success signal by itself.

Every refusal names the stage that stopped processing, in text as a trailing
`[stage=…]` marker and in JSON as `reason.stage`. An adjacent-structure guard
refusal is the primary reason and also reports the underlying raw parse error as
a secondary one, so fixing the guard symptom does not reveal a second failure
later. `--explain` (text only) prints the executed stage order and the whole
failure chain; `--output-format json` exposes the same split as `reason.class`
and `reason.underlying`.

`-` reads UTF-8 stdin. `--stdin-filepath` supplies a virtual name only. Prefer
file input on Windows PowerShell 5.1, whose pipelines/redirection may change
encoding. Strings/comments, BOM and each existing LF/CRLF terminator are
preserved, including mixed files. A new final terminator uses the last observed
style, defaulting to LF. Explicit LF/CRLF conversion refuses to alter line breaks
inside a protected comment/string. Layout includes conservative four-space block indentation,
CST-confirmed operator/call spacing, separators, unprotected trailing whitespace,
blank-line runs and the final newline. Ambiguous indentation is rejected.

Every result is reparsed and checked for token/structure preservation and
idempotence. See [parser design](docs/parser-design.md) and
[coverage/limitations](docs/grammar-coverage.md), [CLI/config](docs/cli.md),
[JSON report contract](docs/report-contract.md) and
[Windows write safety](docs/write-safety.md). The [local Windows performance
baseline](docs/performance-baseline-2026-09-23.md) is informational, not a
release target. See [local packaging and future mise usage](docs/packaging-and-mise.md).
This is not a compiler.

## Known limitations

Read this before you try the tool on a real project.

- **Unreleased alpha.** There is no stable release, no published crate, no git
  tag and no GitHub Release. Nothing here is a supported distribution.
- **No UEFN compiler acceptance has been performed.** Passing tests are
  parser and formatter tests. They are not evidence that UEFN accepts the
  input or the output, and they do not show that formatting preserves behavior
  inside UEFN.
- **What the corpus result means.** The counts are on the `Corpus counts:` line
  at the top of this file, and `tests/docs-drift.rs` fails if they drift from
  `tests/fixtures/corpus.json`. The corpus is self-authored. A golden case
  shows that one small input formats and stays idempotent; a refusal is an
  intended exit 2. Neither says that a real UEFN project parses, so the
  corpus is not a coverage claim about broad projects.
- **Known unsupported constructs** are refused with exit 2 instead of being
  guessed at: structural indentation tabs, `<#>` indented comments, markup
  expressions, quoted or non-ASCII identifiers, multiline string literals
  outside interpolation, unseparated statements on one line, and a type
  annotation continued on the next line. Very deep indentation is refused as
  well. The full table, with the fixture that proves each row, is
  [`docs/grammar-coverage.md`](docs/grammar-coverage.md); it is the source of
  truth, and this list is only a pointer into it.
- **Clean-machine operation is unverified.** Running without Cargo and with only
  the MSVC runtime has not been checked on a machine that lacks the build tools.
- **mise installation is documented but untested.** See
  [packaging and mise](docs/packaging-and-mise.md); no release asset exists for
  mise to install.

## Release gate

An alpha release is not announced until every item below is true. The two
groups differ in who can close them.

Verification gates are re-runnable checks that CI or a maintainer can repeat:

- `pwsh -NoProfile -File scripts/verify.ps1` passes on the commit to release.
- The documentation drift guard (`tests/docs-drift.rs`) passes, so the counts
  and limits quoted in this repository match the code.
- The paired convergence check, run from the sibling linter's checkout, passes,
  so both tools agree on the formatter contract.

Human-owned gates are decisions that CI cannot make and that no workflow in this
repository performs:

- Real UEFN acceptance: a human-run UEFN build of small and multi-file projects
  before and after formatting, with the differences reviewed (plan item F08).
- Distribution readiness: a clean-machine run, checksummed Windows archive, a
  tested mise install and the release procedure (plan item F09).
- The decision to tag, release or publish. CI never does any of these.

The F08 and F09 items are in the maintainer's implementation plan, which is not
part of this repository.

## Related tool

[verse-lint](https://github.com/eiei114/verse-lint) is the sibling linter for
the same language. The two tools are independent: neither depends on the other
at runtime, each pins its own grammar snapshot, and they serve different
purposes (formatting here, diagnostics there). They share a formatter contract
that is checked at development time only.

## Development

Windows x64 is the initial test and distribution target. Rust 1.97.0 (MSVC),
rustfmt and clippy are pinned in `rust-toolchain.toml`. 1.97 is the tested minimum,
not a claim that earlier Rust versions cannot work. Install the MSVC build tools
when building from source. Consumers of future prebuilt binaries will not need Cargo.

```powershell
pwsh -NoProfile -File scripts/verify.ps1
cargo run -- --help
```

`verify.ps1` runs the pinned-toolchain checks plus `scripts/verify-json.ps1`,
which validates real release-binary JSON output for exit 0/1/2 against
`docs/diagnostics.schema.json`, including a write that fails before committing,
an input path given absolutely, and a guard refusal that reports its underlying
parse error as a secondary failure. Both scripts declare `#Requires -Version 7`:
they use `ProcessStartInfo.ArgumentList` and `Test-Json -SchemaFile`, so Windows
PowerShell 5.1 refuses them immediately instead of failing after the Rust build.

`cargo fmt` formats this project's Rust source; `verse-fmt` will format Verse.
Tests and the release build do not replace manual UEFN acceptance. CI never
publishes packages or creates releases. `verse-fmt.dev` is a planned, unregistered
project domain, not an active homepage.

The verification script resolves the pinned toolchain through rustup and forces
its compiler path. This avoids an unrelated mise/direct Rust binary earlier in
PATH silently overriding the repository pin.

The project is community-built and is not affiliated with or endorsed by Epic Games.

## License

Licensed under either of:

- [MIT](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

at your option.
