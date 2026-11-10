param([Parameter(Mandatory = $true)][string]$Version)
$ErrorActionPreference = 'Stop'
$repo = $env:GITHUB_REPOSITORY
if ($repo -ne 'glitchaka/sst-blueversion') {
    throw 'El manifiesto sólo se publica desde el repositorio fuente de SST.'
}
if ([string]::IsNullOrWhiteSpace($env:GH_TOKEN)) {
    throw 'Falta la autorización GITHUB_TOKEN para publicar el canal de SST.'
}
$manifestFile = Join-Path (Split-Path -Parent $PSScriptRoot) 'dist/sst-update.json'
if (-not (Test-Path -LiteralPath $manifestFile -PathType Leaf)) {
    throw "No existe el manifiesto distribuible: $manifestFile"
}
$manifest = Get-Content -Raw -LiteralPath $manifestFile | ConvertFrom-Json
if ($manifest.version -ne $Version -or $manifest.channel -ne 'blue') {
    throw 'La versión o canal del manifiesto no coincide con la publicación.'
}
$releaseText = & gh api "repos/$repo/releases/tags/v$Version"
if ($LASTEXITCODE -ne 0) { throw "La release pública v$Version no existe." }
$release = $releaseText | ConvertFrom-Json
if ($release.draft -or -not $release.published_at) {
    throw "La release v$Version todavía no está publicada."
}
$check = @(
    @{ Name = 'sst.exe'; Hash = $manifest.sha256; Size = $manifest.size; Url = $manifest.asset_url },
    @{ Name = 'sst-updater.exe'; Hash = $manifest.updater_sha256; Size = $manifest.updater_size; Url = $manifest.updater_asset_url },
    @{ Name = "SST-Setup-$Version.exe"; Hash = $manifest.setup.sha256; Size = $manifest.setup.size; Url = $manifest.setup.url }
)
foreach ($expected in $check) {
    $asset = $release.assets | Where-Object { $_.name -eq $expected.Name } | Select-Object -First 1
    if (-not $asset) { throw "La release no contiene $($expected.Name)." }
    if ($asset.digest -ne ('sha256:' + $expected.Hash.ToLowerInvariant())) {
        throw "SHA256 de $($expected.Name) no coincide con el asset publicado."
    }
    if ([int64]$asset.size -ne [int64]$expected.Size -or $asset.browser_download_url -ne $expected.Url) {
        throw "Tamaño o URL de $($expected.Name) no coinciden con la release."
    }
}
$channel = 'updates/blue/sst-update.json'
# The contents API defaults to the repository's main branch. Avoid interpolating
# a PowerShell variable immediately before "?ref": it can produce a malformed URI.
$endpoint = "repos/$repo/contents/$channel"
$currentText = & gh api $endpoint
if ($LASTEXITCODE -ne 0) { throw "No se pudo leer la revisión del canal fuente en $endpoint." }
$current = $currentText | ConvertFrom-Json
if (-not $current.sha) { throw 'El manifiesto actual no dispone de SHA para actualización segura.' }
$previousBytes = [Convert]::FromBase64String(($current.content -replace '\s', ''))
$previous = [Text.Encoding]::UTF8.GetString($previousBytes) | ConvertFrom-Json
$previousNumber = [regex]::Match([string]$previous.version, '-blue\.(\d+)$')
$nextNumber = [regex]::Match($Version, '-blue\.(\d+)$')
if (-not $nextNumber.Success -or
    ($previousNumber.Success -and [int]$previousNumber.Groups[1].Value -gt [int]$nextNumber.Groups[1].Value)) {
    throw "Se rechazó publicar una versión más antigua que $($previous.version)."
}
$bytes = [IO.File]::ReadAllBytes($manifestFile)
$payload = [ordered]@{
    message = "Publish verified SST $Version source update channel"
    content = [Convert]::ToBase64String($bytes)
    sha = $current.sha
    branch = 'main'
} | ConvertTo-Json -Depth 5
$request = Join-Path $env:RUNNER_TEMP 'sst-source-channel.json'
[IO.File]::WriteAllText($request, $payload, [Text.UTF8Encoding]::new($false))
& gh api --method PUT $endpoint --input $request | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'No se pudo publicar el canal de actualización fuente.' }
Write-Host "Canal oficial SST publicado: $Version"
