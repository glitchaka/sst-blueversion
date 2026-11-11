param([Parameter(Mandatory=$true)][string]$UpdaterPath)
$ErrorActionPreference = 'Stop'
$source = [IO.Path]::GetFullPath($UpdaterPath)
if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Falta $source" }
$root = Join-Path $env:RUNNER_TEMP ("sst-updater-e2e-" + [guid]::NewGuid().ToString('N'))
$dir = Join-Path $root 'data\update'
New-Item -Force -ItemType Directory -Path $dir | Out-Null
$worker = Join-Path $dir 'sst-updater.exe'
$target = Join-Path $root 'sst.exe'
$ready = Join-Path $root 'sst.exe.new'
$backup = Join-Path $root 'sst.exe.old'
$pending = Join-Path $dir 'pending.json'
$appliedPending = Join-Path $dir 'applied.pending.json'
$applied = Join-Path $dir 'applied.json'
$names = @('SST_UPDATER_PARENT_PID','SST_UPDATER_TARGET','SST_UPDATER_READY',
    'SST_UPDATER_BACKUP','SST_UPDATER_PENDING','SST_UPDATER_APPLIED_PENDING',
    'SST_UPDATER_APPLIED','SST_UPDATER_STAGED','SST_UPDATER_SHA256')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name,'Process') }
try {
    Copy-Item $source $worker
    Copy-Item (Join-Path $env:SystemRoot 'System32\whoami.exe') $target
    Copy-Item (Join-Path $env:SystemRoot 'System32\where.exe') $ready
    # NTFS alternate streams from the old executable must never move to the
    # fresh verified replacement (ReplaceFileW used to merge them).
    $streamName = 'SST_OLD_ONLY_STREAM'
    Set-Content -LiteralPath $target -Stream $streamName -Value 'OLD_FILE_ONLY'
    $oldSha = (Get-FileHash -Algorithm SHA256 $target).Hash.ToLowerInvariant()
    $newSha = (Get-FileHash -Algorithm SHA256 $ready).Hash.ToLowerInvariant()
    if ($oldSha -eq $newSha) { throw 'Los ejecutables de prueba son idénticos.' }
    '{"pending":true}' | Set-Content $pending
    '{"applied":true}' | Set-Content $appliedPending

    $env:SST_UPDATER_TARGET = $target
    $env:SST_UPDATER_READY = $ready
    $env:SST_UPDATER_BACKUP = $backup
    $env:SST_UPDATER_PENDING = $pending
    $env:SST_UPDATER_APPLIED_PENDING = $appliedPending
    $env:SST_UPDATER_APPLIED = $applied
    $env:SST_UPDATER_STAGED = $ready
    $env:SST_UPDATER_PARENT_PID = [string]$PID
    $env:SST_UPDATER_SHA256 = '0' * 64
    $invalid = Start-Process -FilePath $worker -ArgumentList '--apply' -PassThru -Wait
    if ($invalid.ExitCode -eq 0) { throw 'El updater aceptó un SHA incorrecto.' }
    if ((Get-FileHash -Algorithm SHA256 $target).Hash.ToLowerInvariant() -ne $oldSha) {
        throw 'El fallo de hash dañó SST.'
    }

    $parent = Start-Process -FilePath 'pwsh.exe' -ArgumentList @('-NoLogo','-NoProfile','-Command','Start-Sleep -Seconds 3') -WindowStyle Hidden -PassThru
    $env:SST_UPDATER_PARENT_PID = [string]$parent.Id
    $env:SST_UPDATER_SHA256 = $newSha
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $operation = Start-Process -FilePath $worker -ArgumentList '--apply' -PassThru
    Start-Sleep -Seconds 1
    if ($operation.HasExited) { throw 'El updater no esperó al proceso padre.' }
    if ((Get-FileHash -Algorithm SHA256 $target).Hash.ToLowerInvariant() -ne $oldSha) {
        throw 'Reemplazo prematuro.'
    }
    if (-not $operation.WaitForExit(30000)) { $operation.Kill(); throw 'Updater colgado.' }
    $watch.Stop()
    if ($operation.ExitCode -ne 0) {
        $errorFile = Join-Path $dir 'updater-error.log'
        $details = if (Test-Path $errorFile) { Get-Content -Raw $errorFile } else { 'Sin registro' }
        throw "Updater terminó con error: $details"
    }
    if ($watch.Elapsed.TotalSeconds -lt 2) { throw 'No esperó el cierre del padre.' }
    if ((Get-FileHash -Algorithm SHA256 $target).Hash.ToLowerInvariant() -ne $newSha) {
        throw 'Falló la sustitución del ejecutable.'
    }
    if (-not (Test-Path -Path $backup) -or
        (Get-FileHash -Algorithm SHA256 $backup).Hash.ToLowerInvariant() -ne $oldSha) {
        throw 'No se conservó sst.exe.old.'
    }
    $carried = Get-Item -LiteralPath $target -Stream $streamName -ErrorAction SilentlyContinue
    if ($carried) {
        throw 'El nuevo sst.exe heredó un flujo NTFS del archivo antiguo.'
    }
    $preserved = Get-Item -LiteralPath $backup -Stream $streamName -ErrorAction SilentlyContinue
    if (-not $preserved) {
        throw 'La copia de seguridad perdió los metadatos del ejecutable anterior.'
    }
    if ((Test-Path $pending) -or -not (Test-Path $applied)) {
        throw 'Inconsistencia en el registro de aplicación.'
    }
    Write-Host 'Updater independiente: SHA, espera, sustitución y rollback correctos.'
    # Solo el ejecutable DEBUG admite inyección controlada. El RELEASE nunca
    # reconoce variables de prueba y se valida arriba con un reemplazo real.
    if ($source -match '[\\/]debug[\\/]') {
    # Simula el síntoma real: Windows no puede iniciar el nuevo ejecutable,
    # aunque el fichero descargado pasa el SHA-256. La versión anterior debe
    # restaurarse sin declarar la actualización aplicada.
    Copy-Item (Join-Path $env:SystemRoot 'System32\where.exe') $ready -Force
    '{"pending":true}' | Set-Content $pending
    '{"applied":true}' | Set-Content $appliedPending
    $invalidExeHash = (Get-FileHash -Algorithm SHA256 $ready).Hash.ToLowerInvariant()
    $previousHash = (Get-FileHash -Algorithm SHA256 $target).Hash.ToLowerInvariant()
    $env:SST_UPDATER_SHA256 = $invalidExeHash
    $env:SST_UPDATER_TEST_DENY_LAUNCH = '1'
    $parent = Start-Process -FilePath 'pwsh.exe' -ArgumentList @('-NoLogo','-NoProfile','-Command','Start-Sleep -Seconds 2') -WindowStyle Hidden -PassThru
    $env:SST_UPDATER_PARENT_PID = [string]$parent.Id
    $operation = Start-Process -FilePath $worker -ArgumentList '--apply' -PassThru
    if (-not $operation.WaitForExit(30000)) {
        $operation.Kill()
        throw 'El actualizador quedó bloqueado tras un fallo de ejecución: no se restauró SST a tiempo.'
    }
    if ($operation.ExitCode -eq 0) {
        throw 'El actualizador declaró éxito cuando Windows rechazó el nuevo programa.'
    }
    if ((Get-FileHash -Algorithm SHA256 $target).Hash.ToLowerInvariant() -ne $previousHash) {
        throw 'El fallo de arranque dejó SST inaccesible: no restauró el ejecutable anterior.'
    }
    if (Test-Path -LiteralPath $appliedPending) {
        throw 'Falló la limpieza del recibo pendiente tras restaurar SST.'
    }
    $errorFile = Join-Path $dir 'updater-error.log'
    if ((-not (Test-Path -LiteralPath $errorFile)) -or
        ((Get-Content -Raw -LiteralPath $errorFile) -notmatch 'restaurado autom')) {
        throw 'Falta diagnóstico de rollback al bloquearse el inicio del ejecutable.'
    }
    Remove-Item Env:SST_UPDATER_TEST_DENY_LAUNCH -ErrorAction SilentlyContinue
    Write-Host 'Actualizador: denegación de ejecución simulada y restauración automática verificadas.'
    } else {
        Write-Host 'Release: sustitución real verificada; inyección de acceso denegado cubierta en DEBUG.'
    }

}
finally {
    Remove-Item Env:SST_UPDATER_TEST_DENY_LAUNCH -ErrorAction SilentlyContinue
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
    Remove-Item -LiteralPath $root -Force -Recurse -ErrorAction SilentlyContinue
}
