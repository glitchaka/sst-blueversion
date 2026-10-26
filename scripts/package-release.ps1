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

$release = Join-Path $root 'target\release'
$sst = Join-Path $release 'sst.exe'
$stub = Join-Path $release 'sst_sfx_stub.exe'
if (-not (Test-Path -LiteralPath $sst -PathType Leaf)) {
    throw "Falta $sst. Compile primero el binario release de SST."
}
if (-not (Test-Path -LiteralPath $stub -PathType Leaf)) {
    throw "Falta $stub. Compile primero el stub interno sst_sfx_stub."
}

$dist = Join-Path $root 'dist'
$stage = Join-Path $dist '_payload'
$payloadZip = Join-Path $dist '_payload.zip'
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

Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $payloadZip -CompressionLevel Optimal -Force

$setupName = "SST-Setup-$version.exe"
$setup = Join-Path $dist $setupName
$out = [System.IO.File]::Create($setup)
try {
    $stubStream = [System.IO.File]::OpenRead($stub)
    try { $stubStream.CopyTo($out) } finally { $stubStream.Dispose() }

    $payloadStream = [System.IO.File]::OpenRead($payloadZip)
    try {
        $payloadLength = $payloadStream.Length
        $payloadStream.CopyTo($out)
    } finally {
        $payloadStream.Dispose()
    }

    $magic = [System.Text.Encoding]::ASCII.GetBytes('SSTSFX01')
    $length = [System.BitConverter]::GetBytes([Int64]$payloadLength)
    $out.Write($magic, 0, $magic.Length)
    $out.Write($length, 0, $length.Length)
} finally {
    $out.Dispose()
}

$rawExe = Join-Path $dist 'sst.exe'
Copy-Item -LiteralPath $sst -Destination $rawExe

$exeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $rawExe).Hash.ToLowerInvariant()
$setupHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $setup).Hash.ToLowerInvariant()
$exeSize = (Get-Item -LiteralPath $rawExe).Length
$setupSize = (Get-Item -LiteralPath $setup).Length
if ($setupSize -le (Get-Item -LiteralPath $stub).Length) {
    throw 'El autoextraible final no contiene payload; el tamaño no supera al stub interno.'
}

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
Remove-Item -Force -LiteralPath $payloadZip

Write-Host "SST $version empaquetado en $dist"
Write-Host "  $setupName ($setupSize bytes)"
Write-Host '  sst.exe'
Write-Host '  sst-update.json'
Write-Host '  version.txt'
