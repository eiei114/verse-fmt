param([switch]$Regenerate)
$ErrorActionPreference = 'Stop'
$root = Join-Path (Split-Path -Parent $PSScriptRoot) 'vendor/tree-sitter-verse'
if ($Regenerate) {
    Push-Location $root
    try {
        # Maintainer-only operation. Ordinary Cargo builds do not need Node/npm.
        & npx.cmd --yes tree-sitter-cli@0.25.10 generate --abi 15
        if ($LASTEXITCODE -ne 0) { throw 'Pinned grammar generation failed' }
    } finally { Pop-Location }
}
$manifest = Get-Content (Join-Path $root 'hashes.json') -Raw | ConvertFrom-Json
foreach ($entry in $manifest.PSObject.Properties) {
    $actual = (Get-FileHash -LiteralPath (Join-Path $root $entry.Name) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $entry.Value) { throw "Grammar hash mismatch: $($entry.Name)" }
}
if (-not (Select-String -LiteralPath (Join-Path $root 'src/parser.c') -Pattern '^#define LANGUAGE_VERSION 15$' -Quiet)) {
    throw 'Generated parser must use ABI 15 (metadata file is required)'
}
Write-Output 'Pinned grammar sources, generated artifacts, headers and licenses verified.'
