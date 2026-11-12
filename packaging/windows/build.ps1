<#
.SYNOPSIS
  Builds the Windows installer: target\packaging\packie-<version>-windows-x64-setup.exe

.PARAMETER PackieBin
  packie.exe to package. Default: $env:PACKIE_BIN, else target\release\packie.exe, else the only
  target\<triple>\release\packie.exe.

.PARAMETER ConsoleInstaller
  The console's Tauri NSIS setup. Default: $env:PACKIE_CONSOLE_INSTALLER, else the Tauri output under
  console\src-tauri\target[\<triple>]\release\bundle\nsis; skipped when absent.

.NOTES
  Requires makensis (NSIS 3) on PATH. Not signed.
#>
[CmdletBinding()]
param(
    [string]$PackieBin = $env:PACKIE_BIN,
    [string]$ConsoleInstaller = $env:PACKIE_CONSOLE_INSTALLER
)
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$version = (& (Join-Path $PSScriptRoot '..\version.ps1')).Trim()
if ($version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version '$version' is not MAJOR.MINOR.PATCH; the Windows file version needs four numeric parts."
}

if (-not $PackieBin) {
    $release = Join-Path $root 'target\release\packie.exe'
    if (Test-Path -LiteralPath $release) {
        $PackieBin = $release
    } else {
        $found = @(Get-ChildItem -Path (Join-Path $root 'target\*\release\packie.exe') -ErrorAction SilentlyContinue)
        if ($found.Count -eq 1) {
            $PackieBin = $found[0].FullName
        } elseif ($found.Count -gt 1) {
            throw "Several release binaries found ($($found.FullName -join ', ')); choose one with -PackieBin or `$env:PACKIE_BIN."
        } else {
            throw "No packie.exe found at target\release or target\<triple>\release; build it with 'cargo build --release -p packie' or pass -PackieBin."
        }
    }
}
if (-not (Test-Path -LiteralPath $PackieBin -PathType Leaf)) {
    throw "packie binary not found: $PackieBin (pass -PackieBin or set `$env:PACKIE_BIN)."
}
$PackieBin = (Resolve-Path -LiteralPath $PackieBin).Path

if (-not $ConsoleInstaller) {
    $setup = @(Get-ChildItem -Path @(
            (Join-Path $root 'console\src-tauri\target\release\bundle\nsis\*-setup.exe'),
            (Join-Path $root 'console\src-tauri\target\*\release\bundle\nsis\*-setup.exe')
        ) -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending)
    if ($setup.Count -gt 0) { $ConsoleInstaller = $setup[0].FullName }
} elseif (-not (Test-Path -LiteralPath $ConsoleInstaller -PathType Leaf)) {
    throw "Console installer not found: $ConsoleInstaller"
}

$makensis = Get-Command makensis -ErrorAction SilentlyContinue
if (-not $makensis) { throw 'makensis not found on PATH; install NSIS 3 (https://nsis.sourceforge.io).' }

$outDir = Join-Path $root 'target\packaging'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$outFile = Join-Path $outDir "packie-$version-windows-x64-setup.exe"

$defines = @("/DVERSION=$version", "/DPACKIE_EXE=$PackieBin", "/DOUTFILE=$outFile")
if ($ConsoleInstaller) {
    $ConsoleInstaller = (Resolve-Path -LiteralPath $ConsoleInstaller).Path
    $defines += "/DCONSOLE_INSTALLER=$ConsoleInstaller"
    Write-Host "console: including $ConsoleInstaller"
} else {
    Write-Host 'console: no Tauri NSIS bundle found under console\src-tauri\target\*\bundle\nsis; the installer will not include the console'
}

& $makensis.Source '/V2' @defines (Join-Path $PSScriptRoot 'packie.nsi')
if ($LASTEXITCODE -ne 0) { throw "makensis failed with exit code $LASTEXITCODE" }
Write-Host "built $outFile"
