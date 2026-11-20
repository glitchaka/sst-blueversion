param(
    [string]$OutputDirectory = (Join-Path $PWD 'dist/powershell-module'),
    [string]$PackagePath = (Join-Path $PWD 'dist/SST-PowerShell-win-x64.pkgx')
)
$ErrorActionPreference = 'Stop'
$project = Join-Path $PSScriptRoot '../plugins/powershell/Sst.PowerShell.Bridge.csproj'
if (Test-Path -LiteralPath $OutputDirectory) {
    Remove-Item -LiteralPath $OutputDirectory -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
& dotnet publish $project --configuration Release --runtime win-x64 --self-contained true --output $OutputDirectory -p:PublishSingleFile=false
if ($LASTEXITCODE -ne 0) { throw 'No se pudo construir el SDK PowerShell embebido.' }
foreach ($filename in @(
    'Sst.PowerShell.Bridge.dll',
    'Sst.PowerShell.Bridge.runtimeconfig.json',
    'Sst.PowerShell.Bridge.deps.json',
    'System.Management.Automation.dll',
    'hostfxr.dll',
    'hostpolicy.dll',
    'coreclr.dll'
)) {
    if (-not (Test-Path -LiteralPath (Join-Path $OutputDirectory $filename) -PathType Leaf)) {
        throw "El módulo PowerShell no contiene $filename"
    }
}
& python (Join-Path $PSScriptRoot 'pack-pkgx.py') --id powershell --version 7.5.13 --target win --kind embedded --source $OutputDirectory --output $PackagePath --exclude-dotnet-apphost
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) {
    throw 'No se generó SST-PowerShell-win-x64.pkgx'
}
Write-Host "Módulo PowerShell SST-PACKAGE/1: $PackagePath"
