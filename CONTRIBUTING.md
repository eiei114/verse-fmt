# Contributing

Run `pwsh -NoProfile -File scripts/verify.ps1` before opening a pull request. It
checks the pinned grammar, the parity declaration, formatting, lints, tests and
the Windows release build.

## Changes that require a parity update

`parity.json` declares what `verse-lint` expects from this repository; see
[docs/parity.md](docs/parity.md). Update it, and the linter's `parity.json` in the
same change, when you:

- add, remove or edit a case in `tests/fixtures/corpus.json` (any change moves
  `corpusSha256`, and adding or removing a case moves the golden or refusal count);
- change the vendored grammar, its generated parser or its recorded base revision
  in `vendor/tree-sitter-verse/upstream.md`;
- change CLI, configuration or exit-code behaviour in a way that moves the shared
  toolchain contract version (it moves in both repositories together).

`scripts/verify-parity.ps1` fails, naming the field, when `parity.json` is stale.
Do not add a dependency on `verse-lint`, a submodule or a CI checkout of it, and do
not copy its paired harness here.
