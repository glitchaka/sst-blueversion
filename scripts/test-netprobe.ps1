param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$dir = Join-Path $env:RUNNER_TEMP ('sst-netprobe-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $dir | Out-Null
try {
    $script = Join-Path $dir 'netprobe.sh'
    $stdout = Join-Path $dir 'stdout.txt'
    $stderr = Join-Path $dir 'stderr.txt'
    $commands = @(
        'notsh test --help',
        'echo SST_NETPROBE_HELP_DONE',
        'notsh test 127.0.0.1 --port 1',
        'echo SST_NETPROBE_PROBE_DONE',
        'notsh test loss 127.0.0.1 --count 3 --interval 100 --timeout 200',
        'echo SST_NETPROBE_LOSS_DONE',
        'notsh test stability 127.0.0.1 --seconds 2 --interval 400 --timeout 200',
        'echo SST_NETPROBE_STABILITY_DONE',
        'notsh test speed --passive --seconds 1',
        'echo SST_NETPROBE_SPEED_DONE',
        'notsh test traffic --seconds 1 --top 3',
        'echo SST_NETPROBE_TRAFFIC_DONE',
        'notsh test packets --seconds 1 --top 3',
        'echo SST_NETPROBE_PACKETS_DONE',
        'notsh test capture --interface 0.0.0.0 --seconds 1',
        'echo SST_NETPROBE_CAPTURE_VALIDATION_DONE'
    )
    [IO.File]::WriteAllLines($script, $commands, [Text.UTF8Encoding]::new($false))
    $p = Start-Process -FilePath $exe -ArgumentList ('"' + $script + '"') -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $p.WaitForExit(65000)) {
        $p.Kill()
        throw 'notsh test quedó bloqueado durante una prueba local.'
    }
    $p.Refresh()
    $output = [string](Get-Content -Raw -LiteralPath $stdout)
    $errors = [string](Get-Content -Raw -LiteralPath $stderr)
    if ($p.ExitCode -ne 0) { throw "notsh test falló: $errors / $output" }
    foreach ($expected in @('SST_NETPROBE_HELP_DONE','SST_NETPROBE_PROBE_DONE',
        'DIAGNÓSTICO DE RED','INTERFACES FÍSICAS','RUTA IPv4','DNS','TCP','CONCLUSIÓN',
        'SST_NETPROBE_LOSS_DONE','SST_NETPROBE_STABILITY_DONE','SST_NETPROBE_SPEED_DONE',
        'SST_NETPROBE_TRAFFIC_DONE','SST_NETPROBE_PACKETS_DONE',
        'SST_NETPROBE_CAPTURE_VALIDATION_DONE',
        'PÉRDIDA DE PAQUETES','ESTABILIDAD DE RED','Pérdida:')) {
        if (-not $output.Contains($expected)) {
            throw "Falta etapa de diagnóstico: $expected. Resultado: $output"
        }
    }
    # The Windows runner can use only virtual NICs or deny ETW tracing.
    # Those states must be explicit, never replaced by fake metrics.
    if (-not ($output.Contains('ADAPTADORES FÍSICOS — Windows IP Helper') -or $errors.Contains('no hay interfaces Ethernet/Wi-Fi activas'))) {
        throw "notsh test speed no proporcionó medición ni diagnóstico de ausencia de NIC: $output / $errors"
    }
    if (-not ($output.Contains('TRÁFICO POR PROCESO') -or $errors.Contains('ETW no disponible'))) {
        throw "notsh test traffic no produjo informe ETW ni error verificable: $output / $errors"
    }
    if (-not ($output.Contains('EVENTOS DE RED OBSERVADOS') -or $errors.Contains('ETW no disponible'))) {
        throw "notsh test packets no proporcionó eventos ni explicó la falta de ETW: $output / $errors"
    }
    if ($output -match 'Pérdida: 100.0%' -and $output -notmatch 'Errores locales de la API: 0') {
        throw "notsh test confundió errores de API con pérdida de paquetes."
    }
    if ($output -notmatch 'debe ser una IPv4 local activa' -and $errors -notmatch 'debe ser una IPv4 local activa') {
        throw "notsh test capture aceptó 0.0.0.0 como interfaz o no informó el error esperado: $errors"
    }
    Write-Host 'netprobe: diagnóstico, ICMP pérdida/estabilidad, velocidad de NIC (pasivo) y tráfico ETW verificados.'
} finally {
    if ($p -and -not $p.HasExited) { $p.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $dir -ErrorAction SilentlyContinue
}
