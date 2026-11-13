# The gate every change passes before it is committed: format, clippy with warnings as errors, all tests.
#   .\gate.ps1            run everything
#   .\gate.ps1 -Quick     skip clippy
param([switch]$Quick)
$ErrorActionPreference = 'Continue'   # cargo prints progress on stderr, which PowerShell 5.1 would turn into an error: judge by exit codes instead
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:LOCALAPPDATA\Programs\winlibs\mingw64\bin;" + $env:Path   # the GNU toolchain needs dlltool and gcc from WinLibs
Set-Location $PSScriptRoot
$failed = @()
function Step($name, [scriptblock]$body) {
    Write-Host "== $name" -ForegroundColor Cyan
    & $body
    if ($LASTEXITCODE -ne 0) { $script:failed += $name; Write-Host "FAILED: $name" -ForegroundColor Red }
}
Step 'cargo fmt --check' { cargo fmt --all -- --check }
if (-not $Quick) { Step 'cargo clippy' { cargo clippy --workspace --all-targets -- -D warnings } }
Step 'cargo test' { cargo test --workspace }
if ($failed.Count) { Write-Host ("GATE FAILED: " + ($failed -join ', ')) -ForegroundColor Red; exit 1 }
Write-Host 'GATE PASSED' -ForegroundColor Green
