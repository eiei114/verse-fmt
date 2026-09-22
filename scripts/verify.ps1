# Works in a fresh PowerShell shell. Leaves build artifacts for further inspection.
$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    rustc --version --verbose
    if ($LASTEXITCODE -ne 0) { throw 'Rust toolchain unavailable' }
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt failed' }
    cargo clippy --all-targets --locked -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy failed' }
    cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
    cargo build --release --locked --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Windows release build failed' }
    Write-Output 'Verification passed (UEFN acceptance is a separate gate).'
} finally {
    Pop-Location
}
