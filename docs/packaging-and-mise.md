# Local Windows package and mise example

`scripts/package.py` creates a deterministic, local-only ZIP from a pinned
Windows release binary. It does not create a tag, GitHub Release, crate, or
published artifact. The script refuses dirty source trees and existing output
files; commit the source, run the verification build, then package that exact
checkout:

```powershell
pwsh -NoProfile -File scripts/verify.ps1
python scripts/package.py
```

Output goes under ignored `target/packages/` by default. Archive name follows
`verse-fmt-v<VERSION>-x86_64-pc-windows-msvc.zip`; the executable is at ZIP
root with `README.md`, `BUILD-INFO.txt`, license texts, `NOTICE`, and `docs/`.
A sibling `.sha256` file is generated and verified against the exact archive.
ZIP entry order and timestamps are normalized. Local packaging verifies CRC,
the full entry list and every archived byte. The checksum identifies this
artifact; it is not a signature or trust assertion.
The extracted executable also runs `--version` and a read-only `--check` with
the child process PATH reduced to Windows system directories. This is a local
smoke test only; it is not a clean-machine or MSVC-runtime dependency test.

The following is a future UEFN-project `mise.toml` example only. Version
`0.1.0-alpha.1` has not been released, asset selection has not been tested with
mise, and the domain is not registered. Do not run it expecting installation
to work before an approved GitHub Release exists.

```toml
[tools]
"github:eiei114/verse-fmt" = "0.1.0-alpha.1"

[tasks.fmt]
run = "verse-fmt . --write"

[tasks.check]
run = "verse-fmt . --check"
```

Pin a version; do not use `latest`. `verse.toml` configures the formatter and
is independent of mise. After a release is separately approved, test the
exact asset with a recorded mise version, including install, pin, reinstall,
and checksum behavior. Until then use a directly downloaded, manually
verified archive only for local experiments.
