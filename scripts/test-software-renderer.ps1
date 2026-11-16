param(
    [string]$Exe = '.\\target\\release\\sst.exe',
    [int]$StartupSeconds = 10
)

$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Exe).Path
# Do not force success through the test environment: the installed GUI
# must select the software renderer by itself, even on OpenGL-less machines.
$previousBackend = [Environment]::GetEnvironmentVariable('SLINT_BACKEND', 'Process')
[Environment]::SetEnvironmentVariable('SLINT_BACKEND', $null, 'Process')
$manifest = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\Cargo.toml')
$guiSource = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\presentation\gui_slint.rs')
if ($manifest -notmatch '"renderer-software"' -or $manifest -match '"renderer-femtovg"') {
    throw 'SST debe compilar exclusivamente el renderer-software de Slint.'
}
if ($guiSource -notmatch '\.renderer_name\("software"\.into\(\)\)') {
    throw 'SST no selecciona explícitamente software en su ruta de inicio.'
}
$process = $null
try {
    # Force the GPU-independent Winit path and reject early initialization errors.
    $process = Start-Process -FilePath $exePath -ArgumentList '--gui' -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($StartupSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        if ($process.HasExited) {
            throw "SST no pudo iniciar con renderizado software (exit $($process.ExitCode))."
        }
        if ($process.MainWindowTitle -like '*no pudo iniciarse*') {
            throw "SST mostro un error de inicio: $($process.MainWindowTitle)"
        }
        if ($process.MainWindowTitle -match '^Shell Shock Tool(?: \[.*\])?
            return
        }
    }
    throw "SST no confirmó su ventana nativa en $StartupSeconds segundos."
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
) {
            Write-Host 'OK: SST abrió su ventana nativa con renderer software sin SLINT_BACKEND forzado.'
            return
        }
    }
    throw "SST siguio ejecutandose sin confirmar su ventana nativa en $StartupSeconds segundos."
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
