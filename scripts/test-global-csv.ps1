param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-global-csv-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
try {
    $script = Join-Path $root 'csv.sh'
    $out = Join-Path $root 'stdout.txt'
    $err = Join-Path $root 'stderr.txt'
    $commands = (@('sys info --csv', 'echo __CSV_SYS__', 'whoami --csv',
        'echo __CSV_WHOAMI__', 'help --csv', 'echo __CSV_HELP__') -join [Environment]::NewLine)
    [IO.File]::WriteAllText($script, $commands + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    $process = Start-Process -FilePath $exe -ArgumentList ('"' + $script + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if (-not $process.WaitForExit(30000)) {
        $process.Kill()
        throw 'El modificador CSV dejó bloqueado SST.'
    }
    $process.Refresh()
    if ($process.ExitCode -ne 0) {
        throw "SST salió con código $($process.ExitCode): $(Get-Content -Raw $err)"
    }
    $text = Get-Content -Raw $out
    $header = 'tipo,seccion,fila,campo,valor'
    $count = ([regex]::Matches($text, [regex]::Escape($header))).Count
    if ($count -ne 3) {
        throw "Se esperaban 3 salidas CSV independientes; encontradas: $count. STDOUT: $text"
    }
    foreach ($marker in @('__CSV_SYS__', '__CSV_WHOAMI__', '__CSV_HELP__')) {
        if (-not $text.Contains($marker)) {
            throw "El shell no recuperó el control después del CSV: $marker"
        }
    }
    if ($text -notmatch '(?m)^campo,.*,hostname,') {
        throw "sys info --csv no presenta un campo hostname CSV: $text"
    }
    if ($text -notmatch '(?m)^texto,.*,linea,') {
        throw "La salida no estructurada no se exporta como filas CSV: $text"
    }
    Write-Host 'Global --csv OK: sys info, whoami y help utilizan un único formateador.'
}
finally {
    if ($process -and -not $process.HasExited) { $process.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
