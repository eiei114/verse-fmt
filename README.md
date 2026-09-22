# verse-fmt

Conservative formatter for Epic Games' Verse language and UEFN projects.

> Unreleased alpha under active implementation. Only CLI validation is implemented
> in this foundation commit. Source processing currently fails with exit code 2;
> it does not claim to format or validate Verse. No files are modified.

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

The project is community-built and is not affiliated with or endorsed by Epic Games.

## License

Licensed under either of:

- [MIT](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

at your option.
