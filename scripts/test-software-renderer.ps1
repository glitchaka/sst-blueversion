param(
    [string]$Exe = '.\\target\\release\\sst.exe',
    [int]$StartupSeconds = 10
)

$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Exe).Path
$previousBackend = [Environment]::GetEnvironmentVariable('SLINT_BACKEND', 'Process')
[Environment]::SetEnvironmentVariable('SLINT_BACKEND', 'winit-software', 'Process')
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
        if ($process.MainWindowTitle -eq 'Shell Shock Tool') {
            Write-Host 'OK: SST abrió su ventana nativa con SLINT_BACKEND=winit-software.'
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
