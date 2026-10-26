param(
    [string]$UpdateManifestUrl = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$manifestPath = Join-Path $root 'Cargo.toml'
$manifestText = Get-Content -Raw -LiteralPath $manifestPath
$versionMatch = [regex]::Match($manifestText, '(?m)^version\s*=\s*"([^"]+)"\s*$')
if (-not $versionMatch.Success) {
    throw 'No se pudo obtener la version desde Cargo.toml.'
}
$version = $versionMatch.Groups[1].Value

& (Join-Path $PSScriptRoot 'sync-version.ps1') -Check
if ($LASTEXITCODE -ne 0) {
    throw 'Cargo.toml y Cargo.lock no tienen la misma version.'
}

$release = Join-Path $root 'target\release'
$sst = Join-Path $release 'sst.exe'
$stub = Join-Path $release 'sst_sfx_stub.exe'
$packager = Join-Path $release 'sst_sfx_packager.exe'
foreach ($required in @($sst, $stub, $packager)) {
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
$bg = Join-Path $release 'bg'
if (Test-Path -LiteralPath $bg -PathType Container) {
    Copy-Item -Recurse -LiteralPath $bg -Destination (Join-Path $stage 'bg')
}

if (-not [string]::IsNullOrWhiteSpace($UpdateManifestUrl)) {
    $data = Join-Path $stage 'data'
    New-Item -ItemType Directory -Force -Path $data | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $data 'update-channel.txt'),
        $UpdateManifestUrl.Trim() + [Environment]::NewLine,
        (New-Object System.Text.UTF8Encoding($false))
    )
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
if ($setupSize -le ($stubSize + 16)) {
    throw 'El autoextraible final no contiene payload: su tamano no supera al stub.'
}

$stream = [System.IO.File]::OpenRead($setup)
try {
    if ($stream.Length -lt 16) { throw 'Autoextraible final demasiado pequeno.' }
    $stream.Seek(-16, [System.IO.SeekOrigin]::End) | Out-Null
    $magicBytes = New-Object byte[] 8
    if ($stream.Read($magicBytes, 0, 8) -ne 8) { throw 'No se pudo leer el footer SFX.' }
    $magic = [System.Text.Encoding]::ASCII.GetString($magicBytes)
    if ($magic -ne 'SSTSFX02') { throw "Footer SFX invalido: $magic" }

    $offsetBytes = New-Object byte[] 8
    if ($stream.Read($offsetBytes, 0, 8) -ne 8) { throw 'No se pudo leer el offset del bundle.' }
    $bundleOffset = [System.BitConverter]::ToInt64($offsetBytes, 0)
    if ($bundleOffset -le 0 -or $bundleOffset -ge ($stream.Length - 16)) {
        throw 'Offset del bundle fuera de rango.'
    }
} finally {
    $stream.Dispose()
}

$rawExe = Join-Path $dist 'sst.exe'
Copy-Item -LiteralPath $sst -Destination $rawExe

$exeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $rawExe).Hash.ToLowerInvariant()
$setupHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $setup).Hash.ToLowerInvariant()
$exeSize = (Get-Item -LiteralPath $rawExe).Length

$updateManifest = [ordered]@{
    schema = 1
    version = $version
    channel = 'blue'
    published_at = [DateTimeOffset]::UtcNow.ToString('o')
    asset_url = 'sst.exe'
    sha256 = $exeHash
    size = $exeSize
    setup = [ordered]@{
        name = $setupName
        sha256 = $setupHash
        size = $setupSize
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
Write-Host "  $setupName ($setupSize bytes)"
Write-Host '  sst.exe'
Write-Host '  sst-update.json'
Write-Host '  version.txt'
