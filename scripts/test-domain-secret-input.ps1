param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-domain-secret-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$fixture = Join-Path $root 'domain-arguments.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
# Deliberately fake, no domain operation should ever be attempted.
$sentinel = 'SST_TEST_FAKE_PASSWORD_NOT_A_REAL_SECRET_8Q5T'
$script = @"
sys domain join -d example.invalid -u TEST\operator -p $sentinel
sys domain leave -u TEST\operator --password $sentinel
sys domain -h
echo SST_DOMAIN_PASSWORD_TEST_COMPLETE
"@
[IO.File]::WriteAllText($fixture, $script, [Text.UTF8Encoding]::new($false))
$process = $null
try {
    $process = Start-Process -FilePath $exe -ArgumentList ('"' + $fixture + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if (-not $process.WaitForExit(12000)) {
        $process.Kill()
        throw 'sys domain intentó solicitar credenciales para un valor en línea prohibido.'
    }
    $process.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $out
    $stderr = Get-Content -Raw -LiteralPath $err
    $combined = $stdout + $stderr
    if ($process.ExitCode -ne 0 -or $combined -notmatch 'SST_DOMAIN_PASSWORD_TEST_COMPLETE') {
        throw "No terminó la prueba de dominio (exit $($process.ExitCode))."
    }
    if ($combined.Contains($sentinel)) {
        throw 'La contraseña de prueba apareció en la salida de SST.'
    }
    if ($combined -notmatch 'No escribas la contraseña en el comando') {
        throw 'La ruta de dominio no rechazó contraseñas en la línea de comandos.'
    }
    if ($combined -notmatch 'diálogo protegido') {
        throw 'La ayuda de dominio no describe el ingreso protegido.'
    }
    $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\support\domain.rs')
    if ($source -match '(?s)struct ElevatedPayload\s*\{[^}]*\bpassword\s*:') {
        throw 'La solicitud elevada todavía incluye una contraseña serializada.'
    }
    Write-Host 'Dominio: valores en línea rechazados sin exposición; UAC sin campo secreto.'
}
finally {
    if ($process) {
        $process.Refresh()
        if (-not $process.HasExited) {
            $process.Kill()
        }
    }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
