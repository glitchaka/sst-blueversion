param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-sensors-native-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$fixture = Join-Path $root 'sensors.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
try {
    [IO.File]::WriteAllText($fixture, "sys sensors`necho SST_SENSORS_DONE`n", [Text.UTF8Encoding]::new($false))
    $process = Start-Process -FilePath $exe -ArgumentList ('"' + $fixture + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if (-not $process.WaitForExit(15000)) {
        $process.Kill()
        throw 'sys sensors bloqueó la consola.'
    }
    $process.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $out
    $stderr = Get-Content -Raw -LiteralPath $err
    if ($process.ExitCode -ne 0) {
        throw "sys sensors: salida $($process.ExitCode): $stderr | $stdout"
    }
    if ($stdout -notmatch 'SST SENSORES' -or -not $stdout.Contains('SST_SENSORS_DONE')) {
        throw "No se completó sys sensors: $stdout | $stderr"
    }
    if ($stdout -match 'consulta CIM/WMI excedio|Proveedores no disponibles:\s*\r?\n\s*- WMI/Storage') {
        throw "SST sigue ejecutando la antigua sonda WMI: $stdout"
    }
    $code = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\application\thermal.rs')
    if ($code -match 'powershell\.exe|Get-CimInstance|LibreHardwareMonitor|OpenHardwareMonitor|nvidia-smi\.exe|windows_sensors\(') {
        throw 'El backend térmico mantiene una dependencia rechazada.'
    }
    Write-Host 'sys sensors: respuesta nativa sin PowerShell, WMI ni sondas de terceros.'
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
