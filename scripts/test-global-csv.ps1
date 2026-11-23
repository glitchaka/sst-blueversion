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
    $count = ([regex]::Matches($text, '(?m)^tipo,seccion,fila,campo,valor\r?$')).Count
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

    # Regression: global --csv works before output redirection.
    $redirectScript = Join-Path $root 'csv-redirect.sh'
    $redirectOut = Join-Path $root 'redirect-stdout.txt'
    $redirectErr = Join-Path $root 'redirect-stderr.txt'
    $shellDir = $root.Replace('\', '/')
    $redirectCommands = @(
        ('cd "' + $shellDir + '"'),
        'notsh show lan interfaces --csv > ethernet.csv',
        'notsh routes --csv > routes.csv',
        'sys info --csv > system.csv',
        'echo hola --csv > echo.csv',
        'netcheck --help --csv > netcheck.csv',
        'echo __CSV_REDIRECTS_DONE'
    )
    [IO.File]::WriteAllLines($redirectScript, $redirectCommands, [Text.UTF8Encoding]::new($false))
    $redirectProcess = Start-Process -FilePath $exe -ArgumentList ('"' + $redirectScript + '"') -PassThru -WorkingDirectory $root -RedirectStandardOutput $redirectOut -RedirectStandardError $redirectErr
    if (-not $redirectProcess.WaitForExit(45000)) {
        $redirectProcess.Kill()
        throw 'SST se bloqueó al exportar NOTSH mediante redirección.'
    }
    $redirectProcess.Refresh()
    $redirectText = Get-Content -Raw -LiteralPath $redirectOut
    $redirectErrors = Get-Content -Raw -LiteralPath $redirectErr
    if ($redirectProcess.ExitCode -ne 0 -or -not $redirectText.Contains('__CSV_REDIRECTS_DONE')) {
        throw "SST falló exportando CSV con redirección: $redirectErrors / $redirectText"
    }
    if ($redirectErrors.Contains('no está disponible para este subcomando')) {
        throw "NOTSH sigue rechazando --csv: $redirectErrors"
    }
    foreach ($filename in @('ethernet.csv','routes.csv','system.csv','echo.csv','netcheck.csv')) {
        $file = Join-Path $root $filename
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
            throw "La redirección no creó $filename."
        }
        $content = Get-Content -Raw -LiteralPath $file
        if (-not $content.StartsWith('tipo,seccion,fila,campo,valor' + [char]10)) {
            throw "$filename no contiene una salida CSV normalizada: $content"
        }
        $rows = @(Get-Content -LiteralPath $file | ConvertFrom-Csv)
        if ($rows.Count -lt 1 -or $null -eq $rows[0].campo) {
            throw "$filename no contiene filas CSV legibles."
        }
    }
    if ((Get-Content -Raw (Join-Path $root 'echo.csv')) -notmatch 'texto,,1,linea,hola') {
        throw 'El shell builtin echo no pasó por el modificador CSV global.'
    }
    Write-Host 'CSV universal OK: NOTSH, sys, help, whoami, echo, netcheck y redirecciones reales a disco.'

}
finally {
    if ($process -and -not $process.HasExited) { $process.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
