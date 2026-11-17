param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-domain-secret-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$fixture = Join-Path $root 'domain-arguments.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
$sentinel = 'SST_TEST_FAKE_PASSWORD_NOT_A_REAL_SECRET_8Q5T'

function Run-Command([string]$command, [int]$expectedCode) {
    [IO.File]::WriteAllText($fixture, ($command + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
    Remove-Item -Force -LiteralPath $out, $err -ErrorAction SilentlyContinue
    $process = Start-Process -FilePath $exe -ArgumentList ('"' + $fixture + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    try {
        if (-not $process.WaitForExit(12000)) {
            $process.Kill()
            throw 'sys domain bloqueó la consola con una contraseña prohibida.'
        }
        $process.Refresh()
        $stdout = Get-Content -Raw -LiteralPath $out
        $stderr = Get-Content -Raw -LiteralPath $err
        $combined = $stdout + $stderr
        if ($combined.Contains($sentinel)) {
            throw 'Se filtró el valor sensible en la salida de SST.'
        }
        if ($process.ExitCode -ne $expectedCode) {
            throw "domain: estado $($process.ExitCode), se esperaba $expectedCode."
        }
        return $combined
    }
    finally {
        $process.Refresh()
        if (-not $process.HasExited) { $process.Kill() }
    }
}

try {
    $join = Run-Command "sys domain join -d example.invalid -u TEST\operator -p $sentinel" 2
    $leave = Run-Command "sys domain leave -u TEST\operator --password $sentinel" 2
    if ($join -notmatch 'contraseña en línea prohibida' -or $leave -notmatch 'contraseña en línea prohibida') {
        throw 'La ruta no rechazó el valor en línea antes de ejecutar.'
    }
    $help = Run-Command 'sys domain -h' 0
    if ($help -notmatch 'diálogo protegido') {
        throw 'La ayuda de dominio no describe el ingreso protegido.'
    }
    $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\support\domain.rs')
    if ($source -match '(?s)struct ElevatedPayload\s*\{[^}]*\bpassword\s*:') {
        throw 'El proceso elevado todavía recibe una contraseña serializada.'
    }
    Write-Host 'Dominio: valores en línea rechazados sin exposición; UAC sin campo secreto.'
}
finally {
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
