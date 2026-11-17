<#
.SYNOPSIS
    Fail if an exe imports anything that is not known to exist on Windows 7 /
    Windows Server 2008 R2.

.DESCRIPTION
    A Windows 7 exe that imports a newer function still builds fine, but Windows
    refuses to start it ("entry point not found"). This script lists the exe's
    imports (static and delay-load, via dumpbin) and compares them with
    tools/win7-imports.txt: DLL!Function entries that have been checked to exist
    on Windows 7 SP1.

    If the check fails because of a new import:
      1. Look the function up in Microsoft's docs; "Minimum supported client"
         must be Windows 7 (or older), or Windows Server 2008 R2.
      2. If it is, add it to win7-imports.txt (or run with -Update and review the
         diff). If it isn't, load it at runtime with GetProcAddress and a
         fallback, like the DPI functions in app/src/main.rs.

.EXAMPLE
    ./tools/check-win7-imports.ps1 target/x86_64-win7-windows-msvc/release/small-ups-monitor.exe
#>
param(
    [Parameter(Mandatory = $true)] [string] $Exe,
    [string] $Allowlist,
    # Rewrite the allowlist from this exe. Review the resulting diff before committing.
    [switch] $Update
)
$ErrorActionPreference = 'Stop'
# (Not a parameter default: Windows PowerShell 5.1 leaves $PSScriptRoot empty there.)
if (-not $Allowlist) { $Allowlist = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'win7-imports.txt' }

# DLLs that do not exist on Windows 7 at all (for a clearer error message).
$NotOnWin7 = @(
    'combase.dll', 'shcore.dll', 'api-ms-win-core-synch-l1-2-0.dll', 'api-ms-win-core-path-l1-1-0.dll',
    'api-ms-win-core-winrt-l1-1-0.dll', 'api-ms-win-core-winrt-string-l1-1-0.dll'
)

function Find-Dumpbin {
    $cmd = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path $vswhere) {
        $found = & $vswhere -latest -products * -find 'VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe' | Select-Object -First 1
        if ($found) { return $found }
    }
    throw 'dumpbin.exe not found. Install the Visual Studio C++ build tools.'
}

function Get-Imports([string] $Path) {
    $dumpbin = Find-Dumpbin
    $lines = & $dumpbin /nologo /imports $Path
    if ($LASTEXITCODE -ne 0) { throw "dumpbin failed for $Path" }
    $dll = $null
    $result = New-Object System.Collections.Generic.SortedSet[string] ([StringComparer]::OrdinalIgnoreCase)
    foreach ($line in $lines) {
        if ($line -match '^\s{4}(\S+\.dll)\s*$') {
            $dll = $Matches[1].ToLowerInvariant()
        } elseif ($dll -and $line -match '^\s+Ordinal\s+(\d+)\s*$') {
            [void] $result.Add("$dll!#$($Matches[1])")
        } elseif ($dll -and $line -match '^\s+[0-9A-Fa-f]+\s+([A-Za-z_][\w@?$]*)\s*$') {
            [void] $result.Add("$dll!$($Matches[1])")
        }
    }
    if ($result.Count -eq 0) { throw "No imports parsed from $Path" }
    return , $result
}

if (-not (Test-Path $Exe)) { throw "Exe not found: $Exe" }
$imports = Get-Imports $Exe

if ($Update) {
    $header = @(
        '# Imports of the Windows 7 build (DLL!Function), each checked to exist on Windows 7 SP1 /',
        '# Windows Server 2008 R2. Used by tools/check-win7-imports.ps1 in CI and releases.',
        '# Add an entry only after checking "Minimum supported client" in Microsoft''s docs.'
    )
    ($header + @($imports)) | Set-Content -Path $Allowlist -Encoding ascii
    Write-Host "Wrote $($imports.Count) imports to $Allowlist. Review the diff before committing."
    exit 0
}

$allowed = New-Object System.Collections.Generic.HashSet[string] ([StringComparer]::OrdinalIgnoreCase)
foreach ($line in Get-Content $Allowlist) {
    $entry = $line.Trim()
    if ($entry -and -not $entry.StartsWith('#')) { [void] $allowed.Add($entry) }
}

$bad = @($imports | Where-Object { $NotOnWin7 -contains ($_ -split '!')[0] })
$unknown = @($imports | Where-Object { -not $allowed.Contains($_) -and $bad -notcontains $_ })
$unused = @($allowed | Where-Object { -not $imports.Contains($_) })

Write-Host "$($imports.Count) imports checked against $($allowed.Count) known Windows 7 imports."
foreach ($i in $bad) { Write-Host "::error::$i - this DLL does not exist on Windows 7" }
foreach ($i in $unknown) { Write-Host "::error::$i - new import; confirm it exists on Windows 7, then add it to tools/win7-imports.txt" }
if ($unused.Count) { Write-Host "Not imported any more (can be removed from the list): $($unused -join ', ')" }

if ($bad.Count -or $unknown.Count) {
    Write-Host "Windows 7 import check FAILED: $($bad.Count + $unknown.Count) problem(s)."
    exit 1
}
Write-Host 'Windows 7 import check passed.'
