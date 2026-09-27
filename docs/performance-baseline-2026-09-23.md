# Local Windows performance baseline — 2026-09-23

This is an initial local measurement, not a performance guarantee or CI
threshold. It uses self-authored protected string literals, so it measures
startup, traversal and parser/lexer work on deliberately simple source—not a
representative UEFN project. All runs exited 0, and the script verified that
read-only runs left source hashes unchanged.

## Environment and revisions

- Windows 11 x64, AMD64 Family 26 Model 68 Stepping 0
- Rust 1.97.0, `x86_64-pc-windows-msvc`; PowerShell 7.6.6; Python 3.14.7
- `verse-fmt` revision `ae191de08058a217d331640f0b098d6fb76435f3`
- `verse-lint` revision `8ecb25acfebdf22d08accfcf56ecf145a72bf9cd`
- Release binaries, version `0.1.0-alpha.1`
- Full machine/tool/binary metadata is in the ignored local report
  `target/verification/benchmark/windows-2026-09-23.json`.

Each observation is a separate process invocation, so the reported first and
warm times are both startup-inclusive. Warm is the median of three later
invocations. Private bytes are the maximum sampled every 2 ms while processes
run; short-lived peaks can be missed, so treat memory figures as approximate.

| Input | Files / bytes | Tool | First (ms) | Warm median (ms) | Sampled private bytes |
|---|---:|---|---:|---:|---:|
| Single file | 1 / 1 KiB | `verse-fmt` | 12.87 | 8.96 | 929,792 |
| Single file | 1 / 1 KiB | `verse-lint` | 8.71 | 8.96 | 929,792 |
| Single file | 1 / 100 KiB | `verse-fmt` | 9.73 | 12.09 | 1,507,328 |
| Single file | 1 / 100 KiB | `verse-lint` | 9.73 | 9.51 | 1,196,032 |
| Single file | 1 / 1 MiB | `verse-fmt` | 26.90 | 27.10 | 8,290,304 |
| Single file | 1 / 1 MiB | `verse-lint` | 18.04 | 17.47 | 5,132,288 |
| Project | 100 / 1 MiB | `verse-fmt` | 53.88 | 52.98 | 4,014,080 |
| Project | 100 / 1 MiB | `verse-lint` | 41.87 | 39.58 | 1,232,896 |
| Project | 1,000 / 10 MiB | `verse-fmt` | 400.91 | 399.51 | 30,146,560 |
| Project | 1,000 / 10 MiB | `verse-lint` | 293.95 | 292.48 | 1,732,608 |

The 100- and 1,000-file cases completed with exit 0. The formatter's largest
case remains below one second on this machine. The linter's output was sent to
the null device; JSON serialization cost is included, terminal rendering is
not.

## Rerun

Build the pinned release binaries first in both worktrees. From the formatter
worktree, run:

```powershell
python scripts/benchmark.py `
  --formatter target/x86_64-pc-windows-msvc/release/verse-fmt.exe `
  --linter ..\verse-lint-implementation\target\x86_64-pc-windows-msvc\release\verse-lint.exe `
  --output target/verification/benchmark/windows-local.json `
  --runs 3
```

The report is local verification evidence and is intentionally not committed;
it contains machine identifiers and absolute local binary paths.
