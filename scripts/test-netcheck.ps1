param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$dir = Join-Path $env:RUNNER_TEMP ('sst-netcheck-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $dir | Out-Null
try {
    $script = Join-Path $dir 'netcheck.sh'
    $stdout = Join-Path $dir 'stdout.txt'
    $stderr = Join-Path $dir 'stderr.txt'
    $commands = @('netcheck --help', 'echo SST_NETCHECK_HELP_DONE',
        'netcheck 127.0.0.1 --port 1', 'echo SST_NETCHECK_PROBE_DONE')
    [IO.File]::WriteAllLines($script, $commands, [Text.UTF8Encoding]::new($false))
    $p = Start-Process -FilePath $exe -ArgumentList ('"' + $script + '"') -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $p.WaitForExit(30000)) {
        $p.Kill()
        throw 'netcheck quedó bloqueado durante una prueba local.'
    }
    $p.Refresh()
    $output = Get-Content -Raw -LiteralPath $stdout
    $errors = Get-Content -Raw -LiteralPath $stderr
    if ($p.ExitCode -ne 0) { throw "netcheck falló: $errors / $output" }
    foreach ($expected in @('SST_NETCHECK_HELP_DONE','SST_NETCHECK_PROBE_DONE',
        'DIAGNÓSTICO DE RED','INTERFACES FÍSICAS','RUTA IPv4','DNS','TCP','CONCLUSIÓN')) {
        if (-not $output.Contains($expected)) {
            throw "Falta etapa de diagnóstico: $expected. Resultado: $output"
        }
    }
    Write-Host 'netcheck: consultas Windows nativas y clasificación concluidas.'
} finally {
    if ($p -and -not $p.HasExited) { $p.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $dir -ErrorAction SilentlyContinue
}
