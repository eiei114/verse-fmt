#Requires -Version 7
# Validates parity.json (this repository's side of the paired convergence
# declaration) against reality. It needs no verse-lint binary, no verse-lint
# checkout and no network access; the paired harness is owned by verse-lint.
# -Root and -ParityPath exist so tests can point the check at a tampered copy.
param(
    [string]$Root,
    [string]$ParityPath
)
$ErrorActionPreference = 'Stop'
if (-not $Root) { $Root = Split-Path -Parent $PSScriptRoot }
if (-not $ParityPath) { $ParityPath = Join-Path $Root 'parity.json' }

function Fail([string]$field, [string]$message) {
    [Console]::Error.WriteLine("parity: ${field}: $message")
    exit 1
}

if (-not (Test-Path -LiteralPath $ParityPath -PathType Leaf)) { Fail 'parity.json' "missing: $ParityPath" }
try { $declared = [IO.File]::ReadAllText($ParityPath) | ConvertFrom-Json } catch { Fail 'parity.json' "not valid JSON ($($_.Exception.Message))" }
if ($declared -isnot [pscustomobject]) { Fail 'parity.json' 'must contain a JSON object' }

$required = @('schemaVersion', 'contractVersion', 'corpusPath', 'corpusSha256', 'goldenCaseCount', 'refusalCaseCount', 'formatterGrammarBaseRevision')
$present = @($declared.PSObject.Properties.Name)
# -cnotcontains: key names are case-sensitive.
foreach ($key in $required) { if ($present -cnotcontains $key) { Fail $key 'missing from parity.json' } }
foreach ($key in $present) { if ($required -cnotcontains $key) { Fail $key 'unexpected key in parity.json' } }

function Test-Integer($value) { ($value -is [int] -or $value -is [long]) }
foreach ($key in @('schemaVersion', 'contractVersion', 'goldenCaseCount', 'refusalCaseCount')) {
    if (-not (Test-Integer $declared.$key) -or $declared.$key -lt 0) { Fail $key 'must be a non-negative integer' }
}
if ($declared.schemaVersion -ne 1) { Fail 'schemaVersion' "unsupported value $($declared.schemaVersion)" }
if ($declared.corpusSha256 -isnot [string] -or $declared.corpusSha256 -cnotmatch '^[0-9a-f]{64}$') {
    Fail 'corpusSha256' 'must be 64 lowercase hex digits'
}
if ($declared.formatterGrammarBaseRevision -isnot [string] -or $declared.formatterGrammarBaseRevision -cnotmatch '^[0-9a-f]{40}$') {
    Fail 'formatterGrammarBaseRevision' 'must be a 40-character lowercase revision'
}
if ($declared.corpusPath -isnot [string] -or [string]::IsNullOrWhiteSpace($declared.corpusPath)) {
    Fail 'corpusPath' 'must be a non-empty relative path'
}

# Corpus: path, SHA-256 and the golden/refusal split, all recomputed.
$rootFull = [IO.Path]::GetFullPath($Root)
if ([IO.Path]::IsPathRooted($declared.corpusPath)) { Fail 'corpusPath' "$($declared.corpusPath) must be relative" }
$corpusFile = [IO.Path]::GetFullPath((Join-Path $rootFull $declared.corpusPath))
if (-not $corpusFile.StartsWith($rootFull, [StringComparison]::OrdinalIgnoreCase)) {
    Fail 'corpusPath' "$($declared.corpusPath) must stay inside the repository"
}
if (-not (Test-Path -LiteralPath $corpusFile -PathType Leaf)) { Fail 'corpusPath' "$($declared.corpusPath) does not exist" }
$actualSha = (Get-FileHash -LiteralPath $corpusFile -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualSha -ne $declared.corpusSha256) {
    Fail 'corpusSha256' "declared $($declared.corpusSha256) but $($declared.corpusPath) hashes to $actualSha"
}
try { $corpus = [IO.File]::ReadAllText($corpusFile) | ConvertFrom-Json } catch { Fail 'corpusPath' "$($declared.corpusPath) is not valid JSON" }
$cases = @($corpus.cases)
# A refusal is a case with "reject": true, the same rule tests/docs-drift.rs uses.
$refusals = @($cases | Where-Object { $_.PSObject.Properties.Name -ccontains 'reject' -and $_.reject -eq $true }).Count
$goldens = $cases.Count - $refusals
if ($goldens -ne $declared.goldenCaseCount) {
    Fail 'goldenCaseCount' "declared $($declared.goldenCaseCount) but the corpus has $goldens goldens"
}
if ($refusals -ne $declared.refusalCaseCount) {
    Fail 'refusalCaseCount' "declared $($declared.refusalCaseCount) but the corpus has $refusals refusals"
}

# Contract version: the sources tests/contract-version.rs compares are the
# contract-pair record and the sentence in docs/cli.md.
$pairFile = Join-Path $rootFull 'tests/fixtures/contract-pair.json'
$cliDoc = Join-Path $rootFull 'docs/cli.md'
if (-not (Test-Path -LiteralPath $pairFile -PathType Leaf)) { Fail 'contractVersion' 'tests/fixtures/contract-pair.json is missing' }
if (-not (Test-Path -LiteralPath $cliDoc -PathType Leaf)) { Fail 'contractVersion' 'docs/cli.md is missing' }
$recorded = ([IO.File]::ReadAllText($pairFile) | ConvertFrom-Json).contractVersion
if ($recorded -ne $declared.contractVersion) {
    Fail 'contractVersion' "declared $($declared.contractVersion) but tests/fixtures/contract-pair.json records $recorded"
}
$sentence = [regex]::Match([IO.File]::ReadAllText($cliDoc), 'Shared toolchain contract version\s*\*\*(\d+)\*\*')
if (-not $sentence.Success) { Fail 'contractVersion' 'docs/cli.md does not assert a shared toolchain contract version' }
if ([int]$sentence.Groups[1].Value -ne $declared.contractVersion) {
    Fail 'contractVersion' "declared $($declared.contractVersion) but docs/cli.md asserts $($sentence.Groups[1].Value)"
}

# Grammar base: equal to the vendored snapshot's recorded base, or reachable from HEAD.
$upstream = Join-Path $rootFull 'vendor/tree-sitter-verse/upstream.md'
$snapshot = $null
if (Test-Path -LiteralPath $upstream -PathType Leaf) {
    $m = [regex]::Match([IO.File]::ReadAllText($upstream), '(?m)^- Revision: `([0-9a-f]{40})`')
    if ($m.Success) { $snapshot = $m.Groups[1].Value }
}
$grammarOk = ($snapshot -ceq $declared.formatterGrammarBaseRevision)
if (-not $grammarOk) {
    & git -C $rootFull cat-file -e "$($declared.formatterGrammarBaseRevision)^{commit}" 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) {
        & git -C $rootFull merge-base --is-ancestor $declared.formatterGrammarBaseRevision HEAD 2>&1 | Out-Null
        $grammarOk = ($LASTEXITCODE -eq 0)
    }
}
if (-not $grammarOk) {
    $recordedBase = if ($snapshot) { $snapshot } else { 'none' }
    Fail 'formatterGrammarBaseRevision' "$($declared.formatterGrammarBaseRevision) is neither reachable from HEAD nor the vendored snapshot base ($recordedBase)"
}

Write-Output "Parity declaration verified: corpus $($declared.corpusSha256), $goldens golden and $refusals refusal cases, contract version $($declared.contractVersion), grammar base $($declared.formatterGrammarBaseRevision). Convergence with verse-lint is not claimed; verse-lint owns the paired harness."
