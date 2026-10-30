param(
    [Parameter(Mandatory = $true)]
    [string]$SstPath
)

$ErrorActionPreference = 'Stop'

$sst = [System.IO.Path]::GetFullPath($SstPath)
if (-not (Test-Path -LiteralPath $sst -PathType Leaf)) {
    throw "No existe SST en $sst"
}

$root = Join-Path $env:RUNNER_TEMP ("sst-self-update-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null

try {
    $updateDir = Join-Path $root 'update'
    $helperDir = Join-Path $updateDir 'tmp'
    New-Item -ItemType Directory -Force -Path $helperDir | Out-Null

    $helper = Join-Path $helperDir 'sst-update-helper.exe'
    $target = Join-Path $root 'sst.exe'
    $ready = Join-Path $root 'sst.exe.new'
    $backup = Join-Path $root 'sst.exe.old'
    $pending = Join-Path $updateDir 'pending.json'
    $appliedPending = Join-Path $updateDir 'applied.pending.json'
    $applied = Join-Path $updateDir 'applied.json'

    Copy-Item -LiteralPath $sst -Destination $helper -Force

    $oldExe = Join-Path $env:SystemRoot 'System32\whoami.exe'
    $newExe = Join-Path $env:SystemRoot 'System32\where.exe'
    Copy-Item -LiteralPath $oldExe -Destination $target -Force
    Copy-Item -LiteralPath $newExe -Destination $ready -Force

    '{"pending":true}' | Set-Content -LiteralPath $pending -Encoding utf8NoBOM
    '{"applied":true}' | Set-Content -LiteralPath $appliedPending -Encoding utf8NoBOM

    $expectedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $ready).Hash.ToLowerInvariant()
    $oldHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant()
    if ($oldHash -eq $expectedHash) {
        throw 'La prueba necesita ejecutables viejo/nuevo diferentes.'
    }

    # Simula exactamente la instancia SST que todavía está cerrándose. El helper
    # debe esperar a este PID, aunque el archivo objetivo sea reemplazable antes.
    $parent = Start-Process -FilePath 'pwsh.exe' `
        -ArgumentList @('-NoLogo', '-NoProfile', '-Command', 'Start-Sleep -Seconds 3') `
        -PassThru -WindowStyle Hidden

    $env:SST_INTERNAL_APPLY_UPDATE = '1'
    $env:SST_UPDATE_PARENT_PID = [string]$parent.Id
    $env:SST_UPDATE_TARGET = $target
    $env:SST_UPDATE_READY = $ready
    $env:SST_UPDATE_BACKUP = $backup
    $env:SST_UPDATE_PENDING = $pending
    $env:SST_UPDATE_APPLIED_PENDING = $appliedPending
    $env:SST_UPDATE_APPLIED = $applied
    $env:SST_UPDATE_STAGED = $ready
    $env:SST_UPDATE_SHA256 = $expectedHash

    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $helper -ArgumentList '--gui' -PassThru

    Start-Sleep -Seconds 1
    if ($process.HasExited) {
        throw 'El helper terminó mientras la instancia SST anterior seguía viva.'
    }
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
        throw 'sst.exe desapareció antes de terminar la instancia SST anterior.'
    }
    $duringHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant()
    if ($duringHash -ne $oldHash) {
        throw 'sst.exe fue modificado antes de terminar la instancia SST anterior.'
    }

    if (-not $process.WaitForExit(30000)) {
        try { $process.Kill($true) } catch {}
        throw 'El helper de actualización no terminó dentro de 30 segundos.'
    }
    $timer.Stop()

    if ($process.ExitCode -ne 0) {
        throw "El helper de actualización terminó con código $($process.ExitCode)."
    }
    if ($timer.Elapsed.TotalSeconds -lt 2.0) {
        throw 'El helper no esperó correctamente al proceso SST anterior.'
    }

    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
        throw 'sst.exe desapareció después del reemplazo.'
    }
    $installedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant()
    if ($installedHash -ne $expectedHash) {
        throw "El ejecutable instalado no coincide con el esperado: $installedHash"
    }
    if (Test-Path -LiteralPath $pending) {
        throw 'pending.json no fue eliminado después de aplicar la actualización.'
    }
    if (-not (Test-Path -LiteralPath $applied -PathType Leaf)) {
        throw 'No se generó applied.json después de aplicar la actualización.'
    }
    if (-not (Test-Path -LiteralPath $backup -PathType Leaf)) {
        throw 'No se conservó el rollback sst.exe.old.'
    }
    $backupHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $backup).Hash.ToLowerInvariant()
    if ($backupHash -ne $oldHash) {
        throw 'sst.exe.old no contiene el ejecutable anterior esperado.'
    }

    # Verifica que el ejecutable reemplazado sigue siendo lanzable después del
    # handoff; el problema real no puede dejar una ruta inexistente.
    $probe = Start-Process -FilePath $target -ArgumentList 'cmd.exe' -Wait -PassThru -WindowStyle Hidden
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
        throw 'sst.exe dejó de existir después de volver a lanzarlo.'
    }

    Write-Host "Self-update parent handoff OK ($([math]::Round($timer.Elapsed.TotalSeconds, 2)) s)"
}
finally {
    Remove-Item Env:SST_INTERNAL_APPLY_UPDATE -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_PARENT_PID -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_TARGET -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_READY -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_BACKUP -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_PENDING -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_APPLIED_PENDING -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_APPLIED -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_STAGED -ErrorAction SilentlyContinue
    Remove-Item Env:SST_UPDATE_SHA256 -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 300
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
