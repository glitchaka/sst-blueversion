param(
    [Parameter(Mandatory=$true)][string]$PackagerPath,
    [Parameter(Mandatory=$true)][string]$StubPath
)
$ErrorActionPreference = 'Stop'
$packager = [IO.Path]::GetFullPath($PackagerPath)
$stub = [IO.Path]::GetFullPath($StubPath)
if (-not (Test-Path -LiteralPath $packager -PathType Leaf)) { throw "Falta packager $packager" }
if (-not (Test-Path -LiteralPath $stub -PathType Leaf)) { throw "Falta stub $stub" }

$root = Join-Path $env:RUNNER_TEMP ("sst-sfx-repair-" + [guid]::NewGuid().ToString('N'))
$source = Join-Path $root 'payload'
$installed = Join-Path $root 'existing-sst'
$existingData = Join-Path $installed 'data'
$setup = Join-Path $installed 'SST-Setup-Repair-Test.exe'
New-Item -ItemType Directory -Force -Path $source, $installed, $existingData | Out-Null

try {
    $windowsRoot = if ($env:SystemRoot) { $env:SystemRoot } else { 'C:\Windows' }
    $newExe = Join-Path $windowsRoot 'System32\where.exe'
    $oldExe = Join-Path $windowsRoot 'System32\whoami.exe'
    if (-not (Test-Path -LiteralPath $newExe) -or -not (Test-Path -LiteralPath $oldExe)) {
        throw 'Faltan ejecutables legítimos de prueba de Windows.'
    }

    # The fixture creates a unique package ID. The distribution's actual SFX
    # is not consumed; the real payload can still be released unchanged.
    Copy-Item -LiteralPath $newExe -Destination (Join-Path $source 'sst.exe')
    Copy-Item -LiteralPath $newExe -Destination (Join-Path $source 'sst-updater.exe')
    Copy-Item -LiteralPath $oldExe -Destination (Join-Path $installed 'sst.exe')
    Copy-Item -LiteralPath $oldExe -Destination (Join-Path $installed 'sst-updater.exe')
    [IO.File]::WriteAllText(
        (Join-Path $existingData 'user-config.json'), '{"background":"keep","data":"untouched"}'
    )
    $oldHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $installed 'sst.exe')).Hash
    $newHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $newExe).Hash

    & $packager $source $stub $setup | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'No se pudo generar el paquete de reparación.' }
    if (-not (Test-Path -LiteralPath $setup)) { throw 'Falta autoextraíble de prueba.' }

    # A double-click from the SST folder must repair it without requiring
    # the SST terminal, PowerShell, CMD, or any previously running SST instance.
    $process = Start-Process -FilePath $setup -PassThru
    if (-not $process.WaitForExit(30000)) {
        $process.Kill()
        throw 'SFX quedó detenido al reparar SST.'
    }
    if ($process.ExitCode -ne 0) {
        throw "SFX no pudo reparar una instalación existente (exit=$($process.ExitCode))"
    }
    $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $installed 'sst.exe')).Hash
    if ($actualHash -ne $newHash) {
        throw "SFX no activó el nuevo ejecutable: $actualHash"
    }
    $backups = @(Get-ChildItem -LiteralPath $installed -Filter 'sst.exe.repair.*.bak' -File)
    if ($backups.Count -ne 1 -or
        (Get-FileHash -Algorithm SHA256 -LiteralPath $backups[0].FullName).Hash -ne $oldHash) {
        throw 'La reparación no preservó intacto el sst.exe anterior.'
    }
    if ((Get-Content -Raw -LiteralPath (Join-Path $existingData 'user-config.json')) -notmatch '"background":"keep"') {
        throw 'El autoextraíble modificó los datos o el fondo del usuario.'
    }
    $staging = @(Get-ChildItem -LiteralPath $installed -Filter '.sst-repair-stage-*' -Directory)
    if ($staging.Count -ne 0) {
        throw 'La reparación dejó una extracción temporal en la instalación.'
    }
    Write-Host 'SST SFX: reparación de instalación inaccesible sin consola, respaldo y datos preservados.'
}
finally {
    Remove-Item -LiteralPath $root -Force -Recurse -ErrorAction SilentlyContinue
}
