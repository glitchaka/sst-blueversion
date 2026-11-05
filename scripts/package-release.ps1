$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $root 'Cargo.toml'
$manifestText = Get-Content -Raw -LiteralPath $manifestPath
$versionMatch = [regex]::Match($manifestText, '(?m)^version\s*=\s*"([^"]+)"\s*$')
if (-not $versionMatch.Success) {
    throw 'No se pudo obtener la version desde Cargo.toml.'
}
$version = $versionMatch.Groups[1].Value
$publicReleaseTag = "v$version"
# La release fuente es pública y utiliza GITHUB_TOKEN; no depende de un PAT
# cross-repo cuya ausencia dejó el canal adm-toolbox sin activar.
$publicReleaseBase = "https://github.com/glitchaka/sst-blueversion/releases/download/$publicReleaseTag"
$publicSstUrl = "$publicReleaseBase/sst.exe"
$publicUpdaterUrl = "$publicReleaseBase/sst-updater.exe"
$publicSetupUrl = "$publicReleaseBase/SST-Setup-$version.exe"

$releaseNotesPath = Join-Path $root ".release\v$version.md"
if (-not (Test-Path -LiteralPath $releaseNotesPath -PathType Leaf)) {
    throw "Faltan notas de release para $version en $releaseNotesPath"
}
$changes = @(
    Get-Content -LiteralPath $releaseNotesPath |
        ForEach-Object {
            $match = [regex]::Match($_, '^\s*-\s+(.+?)\s*$')
            if ($match.Success) { $match.Groups[1].Value.Trim() }
        } |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
)
if ($changes.Count -eq 0) {
    throw "Las notas de release de $version no contienen cambios en formato '- texto'."
}
if ($changes.Count -gt 32) {
    throw 'El manifiesto admite como maximo 32 cambios visibles.'
}
foreach ($change in $changes) {
    if ($change.Length -gt 240) {
        throw "Cambio demasiado largo para el manifiesto: $change"
    }
}

& (Join-Path $PSScriptRoot 'sync-version.ps1') -Check
if ($LASTEXITCODE -ne 0) {
    throw 'Cargo.toml y Cargo.lock no tienen la misma version.'
}

$release = Join-Path $root 'target\release'
$sst = Join-Path $release 'sst.exe'
$stub = Join-Path $release 'sst_sfx_stub.exe'
$packager = Join-Path $release 'sst_sfx_packager.exe'
$updater = Join-Path $release 'sst_updater.exe'
foreach ($required in @($sst, $stub, $packager, $updater)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Falta $required. Compile sst, sst_sfx_stub y sst_sfx_packager en release."
    }
}

$dist = Join-Path $root 'dist'
$stage = Join-Path $dist '_payload'
if (Test-Path -LiteralPath $dist) {
    Remove-Item -Recurse -Force -LiteralPath $dist
}
New-Item -ItemType Directory -Force -Path $stage | Out-Null

Copy-Item -LiteralPath $sst -Destination (Join-Path $stage 'sst.exe')
Copy-Item -LiteralPath $updater -Destination (Join-Path $stage 'sst-updater.exe')
$guide = Join-Path $release 'guide'
if (-not (Test-Path -LiteralPath (Join-Path $guide 'guide-index.json') -PathType Leaf)) {
    throw 'Falta la guía local del build: target/release/guide/guide-index.json'
}
Copy-Item -Recurse -LiteralPath $guide -Destination (Join-Path $stage 'guide')
$bg = Join-Path $release 'bg'
if (Test-Path -LiteralPath $bg -PathType Container) {
    Copy-Item -Recurse -LiteralPath $bg -Destination (Join-Path $stage 'bg')
}

$setupName = "SST-Setup-$version.exe"
$setup = Join-Path $dist $setupName
& $packager $stage $stub $setup
if ($LASTEXITCODE -ne 0) {
    throw 'sst_sfx_packager no pudo crear el autoextraible final.'
}
if (-not (Test-Path -LiteralPath $setup -PathType Leaf)) {
    throw "No se genero $setup"
}

$stubSize = (Get-Item -LiteralPath $stub).Length
$setupSize = (Get-Item -LiteralPath $setup).Length
$footerLength = 32
if ($setupSize -le ($stubSize + $footerLength)) {
    throw 'El autoextraible final no contiene payload: su tamano no supera al stub.'
}

$stream = [System.IO.File]::OpenRead($setup)
try {
    if ($stream.Length -lt $footerLength) { throw 'Autoextraible final demasiado pequeno.' }
    $stream.Seek(-$footerLength, [System.IO.SeekOrigin]::End) | Out-Null
    $magicBytes = New-Object byte[] 8
    if ($stream.Read($magicBytes, 0, 8) -ne 8) { throw 'No se pudo leer el footer SFX.' }
    $magic = [System.Text.Encoding]::ASCII.GetString($magicBytes)
    if ($magic -ne 'SSTSFX03') { throw "Footer SFX invalido: $magic" }

    $offsetBytes = New-Object byte[] 8
    if ($stream.Read($offsetBytes, 0, 8) -ne 8) { throw 'No se pudo leer el offset del bundle.' }
    $bundleOffset = [System.BitConverter]::ToInt64($offsetBytes, 0)
    if ($bundleOffset -le 0 -or $bundleOffset -ge ($stream.Length - $footerLength)) {
        throw 'Offset del bundle fuera de rango.'
    }

    $packageIdBytes = New-Object byte[] 16
    if ($stream.Read($packageIdBytes, 0, 16) -ne 16) { throw 'No se pudo leer el ID unico del paquete.' }
    if (($packageIdBytes | Where-Object { $_ -ne 0 }).Count -eq 0) {
        throw 'ID unico del paquete invalido.'
    }
    $packageId = -join ($packageIdBytes | ForEach-Object { $_.ToString('x2') })
} finally {
    $stream.Dispose()
}

$rawExe = Join-Path $dist 'sst.exe'
Copy-Item -LiteralPath $sst -Destination $rawExe
$rawUpdater = Join-Path $dist 'sst-updater.exe'
Copy-Item -LiteralPath $updater -Destination $rawUpdater
# La distribución portable conserva exactamente la misma guía sin Internet.
Copy-Item -Recurse -LiteralPath $guide -Destination (Join-Path $dist 'guide')

$exeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $rawExe).Hash.ToLowerInvariant()
$setupHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $setup).Hash.ToLowerInvariant()
$exeSize = (Get-Item -LiteralPath $rawExe).Length
$updaterHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $rawUpdater).Hash.ToLowerInvariant()
$updaterSize = (Get-Item -LiteralPath $rawUpdater).Length

$updateManifest = [ordered]@{
    schema = 1
    version = $version
    channel = 'blue'
    published_at = [DateTimeOffset]::UtcNow.ToString('o')
    asset_url = $publicSstUrl
    sha256 = $exeHash
    size = $exeSize
    updater_asset_url = $publicUpdaterUrl
    updater_sha256 = $updaterHash
    updater_size = $updaterSize
    changes = $changes
    setup = [ordered]@{
        name = $setupName
        url = $publicSetupUrl
        sha256 = $setupHash
        size = $setupSize
        single_use = $true
        package_id = $packageId
    }
}
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
$manifestJson = ($updateManifest | ConvertTo-Json -Depth 5) + [Environment]::NewLine
[System.IO.File]::WriteAllText(
    (Join-Path $dist 'sst-update.json'),
    $manifestJson,
    $utf8NoBom
)
[System.IO.File]::WriteAllText(
    (Join-Path $dist 'version.txt'),
    $version + [Environment]::NewLine,
    $utf8NoBom
)

Remove-Item -Recurse -Force -LiteralPath $stage

Write-Host "SST $version empaquetado en $dist"
Write-Host "  Cambios publicados: $($changes.Count)"
Write-Host "  Update binario: $publicSstUrl"
Write-Host "  Actualizador independiente: $publicUpdaterUrl"
Write-Host "  $setupName ($setupSize bytes, single-use, id=$packageId)"
Write-Host '  sst.exe + sst-updater.exe + guide/ (portable, offline)'
Write-Host '  sst-update.json'
Write-Host '  version.txt'
