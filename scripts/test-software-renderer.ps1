param(
    [string]$Exe = '.\target\release\sst.exe',
    [int]$StartupSeconds = 12
)

$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Exe).Path

# The application must choose software on its own. Do not force
# SLINT_BACKEND=winit-software from CI; that previously hid the bug.
$previousBackend = [Environment]::GetEnvironmentVariable('SLINT_BACKEND', 'Process')
$process = $null
try {
    [Environment]::SetEnvironmentVariable('SLINT_BACKEND', $null, 'Process')

    $manifest = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\Cargo.toml')
    $guiSource = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\presentation\gui_slint.rs')
    if ($manifest -notmatch '"renderer-software"' -or $manifest -match '"renderer-femtovg"') {
        throw 'SST debe compilar únicamente el renderer-software de Slint.'
    }
    if ($guiSource -notmatch '\.renderer_name\("software"\.into\(\)\)') {
        throw 'SST no selecciona explícitamente software en el arranque.'
    }

    $process = Start-Process -FilePath $exePath -ArgumentList '--gui' -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($StartupSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        if ($process.HasExited) {
            throw "SST no pudo iniciar con software (exit $($process.ExitCode))."
        }
        $title = $process.MainWindowTitle
        if ($title -like '*no pudo iniciarse*') {
            throw "SST mostró un error de arranque: $title"
        }
        if ($title -match '^Shell Shock Tool(?: \[.*\])?$') {
            Write-Host 'OK: SST inició con software sin forzar SLINT_BACKEND.'
            return
        }
    }
    throw "SST no confirmó su ventana principal dentro de $StartupSeconds segundos."
}
finally {
    if ($process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            $process.WaitForExit(5000) | Out-Null
        }
    }
    [Environment]::SetEnvironmentVariable('SLINT_BACKEND', $previousBackend, 'Process')
}
