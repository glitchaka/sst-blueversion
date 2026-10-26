$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Cargo.lock is generated metadata. A previous local build may have updated only
# the root SST version before the synchronized lockfile reached main. Preserve a
# patch before restoring it so git pull can fast-forward cleanly without losing
# evidence of a deliberate local change.
$dirty = git diff --name-only -- Cargo.lock
if ($LASTEXITCODE -ne 0) {
    throw 'No se pudo inspeccionar Cargo.lock.'
}

if ($dirty) {
    $backup = Join-Path $env:TEMP 'sst-Cargo.lock-before-pull.patch'
    git diff -- Cargo.lock | Set-Content -Encoding utf8 -LiteralPath $backup
    git restore -- Cargo.lock
    if ($LASTEXITCODE -ne 0) {
        throw 'No se pudo restaurar Cargo.lock antes del pull.'
    }
    Write-Host "Cargo.lock local restaurado antes del pull. Patch conservado en: $backup"
}
