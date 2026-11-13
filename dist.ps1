# Builds the package you can copy to another Windows 10/11 x64 PC: one folder and one zip under dist\.
#   .\dist.ps1            gate, release build, package, self-test
#   .\dist.ps1 -SkipGate  package what the last gate already passed
param([switch]$SkipGate)
$ErrorActionPreference = 'Stop'
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:LOCALAPPDATA\Programs\winlibs\mingw64\bin;" + $env:Path
Set-Location $PSScriptRoot
# cargo prints progress on stderr, which Windows PowerShell 5.1 turns into an error when errors stop the script.
function Quiet([scriptblock]$body) { $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'; try { & $body 2>$null } finally { $ErrorActionPreference = $old } }

if (-not $SkipGate) {
    & .\gate.ps1
    if ($LASTEXITCODE -ne 0) { throw 'the gate failed: nothing packaged' }
}
# A compiled program carries the paths of its source files, which here include the Windows user folder.
# The package is built separately with those prefixes replaced, so nothing about this PC travels with it.
$home_ = $env:USERPROFILE
$savedFlags = $env:RUSTFLAGS, $env:CFLAGS
$env:RUSTFLAGS = "--remap-path-prefix=$home_=~ --remap-path-prefix=$PSScriptRoot=rdt"
$env:CFLAGS = "-ffile-prefix-map=$home_=~ -ffile-prefix-map=$($home_ -replace '\\', '/')=~"
$ErrorActionPreference = 'Continue'   # cargo writes progress to stderr
cargo build --release --locked --target-dir target\dist
$built = $LASTEXITCODE
$env:RUSTFLAGS, $env:CFLAGS = $savedFlags
if ($built -ne 0) { throw 'release build failed' }
$ErrorActionPreference = 'Stop'

$version = (Quiet { cargo metadata --format-version 1 --no-deps } | ConvertFrom-Json).packages | Where-Object name -eq 'rdt' | Select-Object -ExpandProperty version
$name = "rdt-$version-windows-x64"
$out = Join-Path $PSScriptRoot "dist\$name"
if (Test-Path $out) { Remove-Item $out -Recurse -Force }
New-Item -ItemType Directory -Force $out, "$out\agents" | Out-Null

Copy-Item target\dist\release\rdt.exe $out
Copy-Item LICENSE $out\LICENSE.txt
Copy-Item packaging\QUICKSTART.txt $out
# The agent definition names rdt.exe by a full path on this PC: ship it with a placeholder instead.
$agent = Get-Content docs\agents\reddit-scout.md -Raw
$agent = $agent -replace '[A-Za-z]:/[^`\s]*?/rdt\.exe', 'C:/path/to/rdt.exe' -replace ' \(Dev\\reddit-search\)', ''
[IO.File]::WriteAllText("$out\agents\reddit-scout.md", $agent, (New-Object Text.UTF8Encoding $false))

# Nothing in the package may name this PC's user, in plain or wide text.
$user = $env:USERNAME
foreach ($f in Get-ChildItem $out -Recurse -File) {
    $bytes = [IO.File]::ReadAllBytes($f.FullName)
    foreach ($enc in [Text.Encoding]::ASCII, [Text.Encoding]::Unicode) {
        if ($enc.GetString($bytes).IndexOf($user, [StringComparison]::OrdinalIgnoreCase) -ge 0) { throw "$($f.Name) contains the user name of this PC" }
    }
}

# The exe must need nothing but Windows itself. Any other DLL means it would fail on a PC without our compiler.
$dlls = Quiet { objdump -p "$out\rdt.exe" } | Select-String 'DLL Name: (.+)' | ForEach-Object { $_.Matches[0].Groups[1].Value.Trim().ToLower() } | Sort-Object -Unique
$foreign = $dlls | Where-Object { $_ -notmatch '^(api-ms-win-.*|kernel32|ntdll|advapi32|ws2_32|crypt32|secur32|bcrypt|bcryptprimitives|userenv|ole32|oleaut32|shell32|user32)\.dll$' }
if ($foreign) { throw "the exe needs DLLs that are not part of Windows: $($foreign -join ', ')" }

# Every crate compiled into the exe, with its licence, as the licences ask.
$meta = Quiet { cargo metadata --format-version 1 --locked } | ConvertFrom-Json
$used = Quiet { cargo tree -p rdt --edges normal --prefix none --format '{p}' } | ForEach-Object { ($_ -split ' ')[0..1] -join ' ' } | Sort-Object -Unique
$lines = foreach ($p in $meta.packages | Sort-Object name, version) {
    if ($used -contains "$($p.name) v$($p.version)" -and $p.name -notin 'rdt', 'reddit_core', 'reddit_sources') {
        '{0} {1}  {2}  {3}' -f $p.name, $p.version, $p.license, $p.repository
    }
}
if (-not $dlls) { throw 'could not read the DLL list of the exe' }
if (@($lines).Count -lt 10) { throw 'could not list the components' }
@("rdt $version includes the following open-source components.", 'Each is used under the licence named beside it; the licence texts are at the address given.',
  'SQLite (inside the rusqlite component) is in the public domain.', '') + $lines | Set-Content "$out\THIRD-PARTY.txt" -Encoding utf8

$zip = Join-Path $PSScriptRoot "dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $out -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Set-Content "$zip.sha256" -Encoding ascii

# Self-test: unpack somewhere else and run it with only Windows on PATH and its own empty cache.
$probe = Join-Path $env:TEMP "rdt-dist-probe-$PID"
Expand-Archive $zip -DestinationPath $probe
$savedPath, $savedDb = $env:Path, $env:RDT_DB
try {
    $env:Path = "$env:SystemRoot\System32"
    $env:RDT_DB = Join-Path $probe 'cache.sqlite'
    $exe = Join-Path $probe "$name\rdt.exe"
    $ErrorActionPreference = 'Continue'
    $v = & $exe --version;                                       if ($LASTEXITCODE -ne 0 -or $v -ne "rdt $version") { throw "version check failed: $v" }
    $d = & $exe doctor | Out-String;                             if ($LASTEXITCODE -ne 0 -or $d -notmatch 'full-text search ok') { throw "doctor failed: $d" }
    $r = & $exe resolve 'https://www.reddit.com/r/test/comments/1abc2de/x/' | Out-String; if ($r -notmatch 't3_1abc2de  not cached') { throw "resolve failed: $r" }
    & $exe local 'nothing here' 2>$null | Out-Null;              if ($LASTEXITCODE -ne 3) { throw "an empty cache should exit 3, got $LASTEXITCODE" }
    $ErrorActionPreference = 'Stop'
} finally {
    $env:Path, $env:RDT_DB = $savedPath, $savedDb
    Remove-Item $probe -Recurse -Force -ErrorAction SilentlyContinue
}

'{0}  ({1:N1} MB exe, {2:N1} MB zip, {3} components listed)' -f $name, ((Get-Item "$out\rdt.exe").Length / 1MB), ((Get-Item $zip).Length / 1MB), $lines.Count
"zip     $zip"
"sha256  $hash"
'self-test passed: ran from a clean folder with only Windows on PATH (no network used)'
exit 0
