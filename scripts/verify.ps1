# Works in a fresh PowerShell shell. Leaves build artifacts for further inspection.
$ErrorActionPreference = 'Stop'
$saved = @{ Path = $env:Path; RUSTUP_TOOLCHAIN = $env:RUSTUP_TOOLCHAIN; RUSTC = $env:RUSTC; RUSTDOC = $env:RUSTDOC }
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    $match = Select-String -Path rust-toolchain.toml -Pattern '^channel\s*=\s*"([^"]+)"'
    if (-not $match) { throw 'No pinned Rust channel found' }
    $channel = $match.Matches[0].Groups[1].Value
    $installed = @(rustup toolchain list)
    if ($LASTEXITCODE -ne 0) { throw 'rustup unavailable' }
    if (-not ($installed | Where-Object { $_.StartsWith("$channel-") })) {
        rustup toolchain install $channel --profile minimal
        if ($LASTEXITCODE -ne 0) { throw 'Cannot install pinned toolchain' }
    }
    rustup component add --toolchain $channel rustfmt clippy
    if ($LASTEXITCODE -ne 0) { throw 'Pinned Rust components unavailable' }
    $rustc = (rustup which --toolchain $channel rustc)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve pinned compiler' }
    $bin = Split-Path -Parent $rustc.Trim()
    # mise/direct Rust installations may precede rustup proxies on PATH.
    $env:Path = $bin + [IO.Path]::PathSeparator + $saved.Path
    $env:RUSTUP_TOOLCHAIN = $channel
    $env:RUSTC = Join-Path $bin 'rustc.exe'
    $env:RUSTDOC = Join-Path $bin 'rustdoc.exe'
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
    foreach ($key in $saved.Keys) { [Environment]::SetEnvironmentVariable($key, $saved[$key], 'Process') }
    Pop-Location
}
