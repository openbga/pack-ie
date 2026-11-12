# Prints the PACK-IE version: `[workspace.package] version` of the workspace Cargo.toml.
# Same logic as packaging/version.sh.
$ErrorActionPreference = 'Stop'

$manifest = Join-Path (Split-Path -Parent $PSScriptRoot) 'Cargo.toml'
$section = $false
foreach ($line in Get-Content -LiteralPath $manifest) {
    if ($line -match '^\[') { $section = ($line -match '^\[workspace\.package\]\s*$'); continue }
    if ($section -and $line -match '^version\s*=\s*"([^"]+)"') {
        Write-Output $Matches[1]
        return
    }
}
throw "version.ps1: no [workspace.package] version found in $manifest"
