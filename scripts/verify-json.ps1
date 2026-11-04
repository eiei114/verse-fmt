#Requires -Version 7
# Validates real release-binary JSON output against docs/diagnostics.schema.json.
# Covers exit 0/1/2, every status, a Unicode-and-space path, determinism, the
# forbidden flag combinations, the unchanged text default, and the two failure
# modes this contract must not misreport: a write that never committed and a
# diagnostic built from an absolute path. Windows PowerShell 5.1 cannot run
# this: it lacks ProcessStartInfo.ArgumentList and Test-Json -SchemaFile.
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$bin = Join-Path $repo 'target/x86_64-pc-windows-msvc/release/verse-fmt.exe'
if (-not (Test-Path -LiteralPath $bin)) {
    $bin = Join-Path $repo 'target/release/verse-fmt.exe'
}
if (-not (Test-Path -LiteralPath $bin)) { throw "Release binary not found; run the release build first" }
$schema = Join-Path $repo 'docs/diagnostics.schema.json'
if (-not (Test-Path -LiteralPath $schema)) { throw 'docs/diagnostics.schema.json is missing' }

$workspace = Join-Path ([IO.Path]::GetTempPath()) ("verse-fmt-json-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $workspace | Out-Null

function Invoke-VerseFmt {
    param([string[]]$Arguments, [string]$WorkingDirectory)
    $start = [Diagnostics.ProcessStartInfo]::new($bin)
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $start.WorkingDirectory = $WorkingDirectory
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.UseShellExecute = $false
    $process = [Diagnostics.Process]::Start($start)
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    [pscustomobject]@{ Stdout = $stdout; Stderr = $stderr; ExitCode = $process.ExitCode }
}

function Assert-Envelope {
    param([pscustomobject]$Result, [string]$What)
    if ($Result.ExitCode -notin @(0, 1, 2)) { throw "$What`: unexpected exit code $($Result.ExitCode)" }
    $text = $Result.Stdout
    if ([string]::IsNullOrWhiteSpace($text)) { throw "$What`: no JSON on stdout" }
    if (-not ($text.Trim() | Test-Json -SchemaFile $schema -ErrorAction SilentlyContinue)) {
        throw "$What`: stdout does not validate against docs/diagnostics.schema.json`n$text"
    }
    if ($text.Contains([char]27)) { throw "$What`: JSON contains an ANSI escape" }
    if ($text.Contains($workspace)) { throw "$What`: JSON contains an absolute path" }
    if ($text.Contains('\')) { throw "$What`: JSON contains a backslash path separator" }
    if ($text -match '\d{4}-\d\d-\d\dT') { throw "$What`: JSON contains a timestamp" }
    $parsed = @($text | ConvertFrom-Json)
    if ($parsed.Count -ne 1) { throw "$What`: stdout is not exactly one JSON object" }
    $parsed[0]
}

function Assert-Equal {
    param($Actual, $Expected, [string]$What)
    if ("$Actual" -ne "$Expected") { throw "$What`: expected '$Expected', got '$Actual'" }
}

try {
    # ---- a project with all three statuses --------------------------------
    $project = Join-Path $workspace '日本語 project'
    New-Item -ItemType Directory (Join-Path $project 'nested folder') | Out-Null
    Set-Content -LiteralPath (Join-Path $project 'nested folder/ok.verse') -Value "Value := 1`n" -NoNewline
    Set-Content -LiteralPath (Join-Path $project 'nested folder/needs.verse') -Value 'Other:=2' -NoNewline
    Set-Content -LiteralPath (Join-Path $project 'nested folder/broken.verse') -Value '<# unterminated' -NoNewline

    $check = Invoke-VerseFmt @('.', '--check', '--output-format', 'json') $project
    Assert-Equal $check.ExitCode 2 'check with a refused file'
    $report = Assert-Envelope $check 'check with a refused file'
    Assert-Equal $report.schemaVersion 1 'schemaVersion'
    Assert-Equal $report.tool.name 'verse-fmt' 'tool.name'
    Assert-Equal $report.summary.filesChecked 3 'summary.filesChecked'
    Assert-Equal $report.summary.filesChanged 1 'summary.filesChanged'
    Assert-Equal $report.summary.filesUnchanged 1 'summary.filesUnchanged'
    Assert-Equal $report.summary.filesRefused 1 'summary.filesRefused'
    Assert-Equal $report.summary.complete 'False' 'summary.complete'
    $byPath = @{}
    foreach ($entry in $report.files) { $byPath[$entry.path] = $entry }
    Assert-Equal $byPath['nested folder/ok.verse'].status 'unchanged' 'unchanged status'
    Assert-Equal $byPath['nested folder/needs.verse'].status 'would-format' 'would-format status'
    Assert-Equal $byPath['nested folder/needs.verse'].reason.stage 'layout' 'would-format reason stage'
    Assert-Equal $byPath['nested folder/broken.verse'].status 'refused' 'refused status'
    Assert-Equal $byPath['nested folder/broken.verse'].reason.stage 'lexer' 'refused reason stage'
    Assert-Equal $report.executionErrors.Count 1 'executionErrors count'
    Assert-Equal $report.executionErrors[0].path 'nested folder/broken.verse' 'executionErrors path'
    if (-not $check.Stderr.Contains('broken.verse')) { throw 'refusal did not also reach stderr' }
    if ($check.Stderr.Contains([char]27)) { throw 'stderr contains an ANSI escape for a redirected stream' }

    # ---- exit 1 for pending changes only ---------------------------------
    Remove-Item -LiteralPath (Join-Path $project 'nested folder/broken.verse')
    $pending = Invoke-VerseFmt @('.', '--check', '--output-format', 'json') $project
    Assert-Equal $pending.ExitCode 1 'check with pending changes'
    $report = Assert-Envelope $pending 'check with pending changes'
    Assert-Equal $report.summary.complete 'True' 'complete when nothing failed'
    Assert-Equal $report.executionErrors.Count 0 'executionErrors empty on success'

    # ---- determinism ------------------------------------------------------
    $again = Invoke-VerseFmt @('.', '--check', '--output-format', 'json', '--color', 'always') $project
    $textPending = Invoke-VerseFmt @('.', '--check') $project
    Assert-Equal $textPending.ExitCode $pending.ExitCode 'text and json agree on the exit code'
    if ($again.Stdout -cne $pending.Stdout) { throw 'two identical runs produced different JSON bytes' }

    # ---- exit 0 with --write, reporting the post-write state --------------
    $write = Invoke-VerseFmt @('.', '--write', '--output-format', 'json') $project
    Assert-Equal $write.ExitCode 0 'write'
    $report = Assert-Envelope $write 'write'
    $byPath = @{}
    foreach ($entry in $report.files) { $byPath[$entry.path] = $entry }
    Assert-Equal $byPath['nested folder/needs.verse'].status 'formatted' 'formatted status after write'
    Assert-Equal $byPath['nested folder/ok.verse'].status 'unchanged' 'unchanged status after write'
    $written = (Get-Content -LiteralPath (Join-Path $project 'nested folder/needs.verse') -Raw) -replace "`r?`n$", ''
    Assert-Equal $written 'Other := 2' 'written bytes'
    if (-not $write.Stderr.Contains('changed 1')) { throw 'human write summary missing from stderr' }

    # ---- every status is reachable, and exit 2 comes back ----------------
    Set-Content -LiteralPath (Join-Path $project 'nested folder/broken.verse') -Value '<# unterminated' -NoNewline
    $failure = Invoke-VerseFmt @('.', '--check', '--output-format', 'json') $project
    Assert-Equal $failure.ExitCode 2 'refusal reinstated'
    Assert-Envelope $failure 'refusal reinstated' | Out-Null

    # ---- forbidden combinations ------------------------------------------
    foreach ($combo in @(
            @(@('.', '--diff', '--output-format', 'json'), '--diff'),
            @(@('--show-config', '--output-format', 'json'), '--show-config')
        )) {
        $result = Invoke-VerseFmt $combo[0] $project
        Assert-Equal $result.ExitCode 2 "json with $($combo[1])"
        if ($result.Stdout -ne '') { throw "json with $($combo[1]) still wrote to stdout" }
        if (-not $result.Stderr.Contains('cannot be combined')) {
            throw "json with $($combo[1]) did not explain the conflict"
        }
    }

    # ---- a write failure is an execution error, not a refused file -------
    # Preflight refuses the whole batch, so nothing may claim `formatted` and no
    # entry may be `refused`: a refusal means the source was rejected.
    $writeProject = Join-Path $workspace 'write failure'
    New-Item -ItemType Directory $writeProject | Out-Null
    Set-Content -LiteralPath (Join-Path $writeProject 'a.verse') -Value 'A:=1' -NoNewline
    Set-Content -LiteralPath (Join-Path $writeProject 'z.verse') -Value 'Z:=2' -NoNewline
    $protected = Get-Item -LiteralPath (Join-Path $writeProject 'z.verse')
    $protected.IsReadOnly = $true
    try {
        $blocked = Invoke-VerseFmt @('.', '--write', '--output-format', 'json') $writeProject
        Assert-Equal $blocked.ExitCode 2 'write failure'
        $report = Assert-Envelope $blocked 'write failure'
        Assert-Equal $report.summary.filesChecked 2 'write failure filesChecked'
        Assert-Equal $report.summary.filesRefused 0 'write failure filesRefused'
        Assert-Equal $report.summary.complete 'False' 'write failure complete'
        foreach ($entry in $report.files) {
            Assert-Equal $entry.status 'would-format' "write failure status of $($entry.path)"
        }
        Assert-Equal $report.executionErrors.Count 1 'write failure executionErrors count'
        Assert-Equal $report.executionErrors[0].path 'z.verse' 'write failure executionErrors path'
        if (-not $report.executionErrors[0].message.Contains('read-only file')) {
            throw 'write failure did not explain the refusal'
        }
        $unchanged = (Get-Content -LiteralPath (Join-Path $writeProject 'a.verse') -Raw) -replace "`r?`n$", ''
        Assert-Equal $unchanged 'A:=1' 'a write failure must not replace another file'
    } finally {
        $protected.IsReadOnly = $false
    }

    # ---- an absolute input path does not reach the machine report -------
    $absolute = Join-Path $workspace 'absent folder/missing.verse'
    $absoluteResult = Invoke-VerseFmt @($absolute, '--check', '--output-format', 'json') $project
    Assert-Equal $absoluteResult.ExitCode 2 'absolute missing path'
    $report = Assert-Envelope $absoluteResult 'absolute missing path'
    if (-not ($report.executionErrors | Where-Object { $_.message.Contains('absent folder/missing.verse') })) {
        throw 'the affected source is no longer identifiable in the machine report'
    }
    if ($report.executionErrors | Where-Object { $null -ne $_.path }) {
        throw 'a discovery failure must not be attributed to a file entry'
    }

    # ---- the default stays byte-identical text ---------------------------
    $text = Invoke-VerseFmt @('.', '--check') $project
    $explicit = Invoke-VerseFmt @('.', '--check', '--output-format', 'text') $project
    Assert-Equal $explicit.ExitCode $text.ExitCode 'explicit text exit code'
    if ($explicit.Stdout -cne $text.Stdout) { throw '--output-format text changed stdout' }
    if ($explicit.Stderr -cne $text.Stderr) { throw '--output-format text changed stderr' }

    Write-Output 'JSON report contract verified against the release binary.'
} finally {
    Remove-Item -Recurse -Force $workspace -ErrorAction SilentlyContinue
}
