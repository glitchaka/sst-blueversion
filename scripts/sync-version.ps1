param(
    [switch]$Check
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $root 'Cargo.toml'
$cargoLock = Join-Path $root 'Cargo.lock'

$manifest = Get-Content -Raw -LiteralPath $cargoToml
$match = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"\s*$')
if (-not $match.Success) {
    throw 'No se pudo obtener la version de SST desde Cargo.toml.'
}
$version = $match.Groups[1].Value

$lock = Get-Content -Raw -LiteralPath $cargoLock
$pattern = '(?ms)(\[\[package\]\]\r?\nname = "sst"\r?\nversion = ")[^"]+("\r?\ndependencies = \[)'
$lockMatch = [regex]::Match($lock, $pattern)
if (-not $lockMatch.Success) {
    throw 'No se encontro el paquete raiz sst en Cargo.lock.'
}
$lockedVersion = [regex]::Match($lockMatch.Value, 'version = "([^"]+)"').Groups[1].Value

if ($lockedVersion -eq $version) {
    Write-Host "SST version OK: $version"
    exit 0
}

if ($Check) {
    throw "Version inconsistente: Cargo.toml=$version, Cargo.lock=$lockedVersion"
}

$replacement = '${1}' + $version + '${2}'
$updated = [regex]::Replace($lock, $pattern, $replacement, 1)
[System.IO.File]::WriteAllText($cargoLock, $updated, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Cargo.lock sincronizado: $lockedVersion -> $version"
