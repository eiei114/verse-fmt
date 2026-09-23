# verse-fmt

Conservative formatter for Epic Games' Verse language and UEFN projects.

> Unreleased alpha under active implementation. File/stdin/project formatting,
> configuration, check, diff and guarded Windows writes work for a bounded
> syntax subset. The 71-case self-authored corpus and local performance baseline
> pass, but UEFN compiler acceptance, broad-project readiness and distribution
> checks are still pending. Unsupported syntax fails with exit code 2.

## Usage

```powershell
cargo run -- path/to/device.verse          # formatted UTF-8 to stdout
cargo run -- path/to/device.verse --check  # exit 0 clean, 1 needs formatting, 2 failure
cargo run -- . --diff                     # preview project changes
cargo run -- . --write                    # modify eligible sources after preflight
cargo run -- --show-config                # effective verse.toml settings
```

`-` reads UTF-8 stdin. `--stdin-filepath` supplies a virtual name only. Prefer
file input on Windows PowerShell 5.1, whose pipelines/redirection may change
encoding. Strings/comments, BOM and LF/CRLF are preserved; mixed line endings
are rejected. Layout includes conservative four-space block indentation,
CST-confirmed operator/call spacing, separators, unprotected trailing whitespace,
blank-line runs and the final newline. Ambiguous indentation is rejected.

Every result is reparsed and checked for token/structure preservation and
idempotence. See [parser design](docs/parser-design.md) and
[coverage/limitations](docs/grammar-coverage.md), [CLI/config](docs/cli.md) and
[Windows write safety](docs/write-safety.md). The [local Windows performance
baseline](docs/performance-baseline-2026-09-23.md) is informational, not a
release target. See [local packaging and future mise usage](docs/packaging-and-mise.md).
This is not a compiler.

## Development

Windows x64 is the initial test and distribution target. Rust 1.97.0 (MSVC),
rustfmt and clippy are pinned in `rust-toolchain.toml`. 1.97 is the tested minimum,
not a claim that earlier Rust versions cannot work. Install the MSVC build tools
when building from source. Consumers of future prebuilt binaries will not need Cargo.

```powershell
./scripts/verify.ps1
cargo run -- --help
```

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
