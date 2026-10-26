param(
    [switch]$SkipPull,
    [string]$UpdateManifestUrl = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Get-SstVersion {
    $text = Get-Content -Raw -LiteralPath (Join-Path $root 'Cargo.toml')
    $m = [regex]::Match($text, '(?m)^version\s*=\s*"([^"]+)"\s*$')
    if (-not $m.Success) { throw 'No se pudo leer la version de SST desde Cargo.toml.' }
    return $m.Groups[1].Value
}

function Restore-StaleVersionOnlyLockChange {
    $names = @(git diff --name-only)
    if ($LASTEXITCODE -ne 0) { throw 'No se pudo inspeccionar el estado local de Git.' }
    if ($names.Count -ne 1 -or $names[0] -ne 'Cargo.lock') { return }

    $diff = @(git diff --unified=0 -- Cargo.lock)
    if ($LASTEXITCODE -ne 0) { return }
    $changed = @($diff | Where-Object {
        ($_ -match '^[+-]') -and
        ($_ -notmatch '^\+\+\+') -and
        ($_ -notmatch '^---')
    })

    if ($changed.Count -eq 2 -and
        $changed[0] -match '^-version = "[^"]+"$' -and
        $changed[1] -match '^\+version = "[^"]+"$') {
        Write-Host 'Cargo.lock solo contiene un desfase local de version; se restaura antes del pull.'
        git restore -- Cargo.lock
        if ($LASTEXITCODE -ne 0) { throw 'No se pudo restaurar Cargo.lock.' }
    }
}

Write-Host '============================================'
Write-Host ' SST Blue Version - Update + Build + SFX'
Write-Host '============================================'
Write-Host ''

if (-not $SkipPull) {
    Write-Host '[1/4] Actualizando repositorio...'
    Restore-StaleVersionOnlyLockChange
    git pull --ff-only
    if ($LASTEXITCODE -ne 0) {
        throw 'git pull fallo. No se modificaron ni descartaron cambios locales ajenos al desfase de version de Cargo.lock.'
    }
} else {
    Write-Host '[1/4] Pull omitido.'
}

Write-Host ''
Write-Host '[2/4] Verificando version canonica...'
& (Join-Path $PSScriptRoot 'sync-version.ps1') -Check
if ($LASTEXITCODE -ne 0) { throw 'La version de Cargo.toml y Cargo.lock no coincide.' }
$version = Get-SstVersion
Write-Host "SST $version"

Write-Host ''
Write-Host '[3/4] Compilando SST y stub interno del autoextraible...'
cargo build --release --bin sst --bin sst_sfx_stub
if ($LASTEXITCODE -ne 0) { throw 'La compilacion release fallo.' }

Write-Host ''
Write-Host '[4/4] Generando autoextraible final...'
& (Join-Path $PSScriptRoot 'package-release.ps1') -UpdateManifestUrl $UpdateManifestUrl
if ($LASTEXITCODE -ne 0) { throw 'El empaquetado del autoextraible fallo.' }

$setup = Join-Path $root "dist\SST-Setup-$version.exe"
$stub = Join-Path $root 'target\release\sst_sfx_stub.exe'
if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) {
    throw "No se genero $setup"
}
if (-not (Test-Path -LiteralPath $stub -PathType Leaf)) {
    throw "No existe el stub interno $stub"
}

$setupLength = (Get-Item -LiteralPath $setup).Length
$stubLength = (Get-Item -LiteralPath $stub).Length
if ($setupLength -le $stubLength + 16) {
    throw 'El archivo final no contiene un payload SST valido: su tamaño no supera al stub interno.'
}

$stream = [System.IO.File]::OpenRead($setup)
try {
    if ($stream.Length -lt 16) { throw 'Autoextraible final demasiado pequeno.' }
    $stream.Seek(-16, [System.IO.SeekOrigin]::End) | Out-Null
    $footer = New-Object byte[] 8
    if ($stream.Read($footer, 0, 8) -ne 8) { throw 'No se pudo leer el footer del SFX.' }
    $magic = [System.Text.Encoding]::ASCII.GetString($footer)
    if ($magic -ne 'SSTSFX01') { throw "Footer SFX invalido: $magic" }

    $sizeBytes = New-Object byte[] 8
    if ($stream.Read($sizeBytes, 0, 8) -ne 8) { throw 'No se pudo leer el tamano del payload SFX.' }
    $payloadLength = [System.BitConverter]::ToInt64($sizeBytes, 0)
    if ($payloadLength -le 0 -or $payloadLength -gt ($stream.Length - 16)) {
        throw 'Longitud de payload SFX invalida.'
    }
} finally {
    $stream.Dispose()
}

$sizeMiB = [Math]::Round($setupLength / 1MB, 2)
$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $setup).Hash.ToLowerInvariant()

Write-Host ''
Write-Host '============================================'
Write-Host ' BUILD COMPLETO'
Write-Host " Version       : $version"
Write-Host " Autoextraible : $setup"
Write-Host " Tamano        : $sizeMiB MiB"
Write-Host " SHA-256       : $hash"
Write-Host '============================================'
