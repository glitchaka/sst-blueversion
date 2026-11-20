param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-instp-check-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$scriptFile = Join-Path $root 'instp.sh'
$stdoutFile = Join-Path $root 'stdout.txt'
$stderrFile = Join-Path $root 'stderr.txt'
try {
    $commands = @'
instp sst
echo SST_INSTP_STATUS_RETURNED
instp help
echo SST_INSTP_HELP_RETURNED
'@
    [IO.File]::WriteAllText($scriptFile, $commands, [Text.UTF8Encoding]::new($false))
    $process = Start-Process -FilePath $exe -ArgumentList ('"' + $scriptFile + '"') -PassThru -RedirectStandardOutput $stdoutFile -RedirectStandardError $stderrFile
    if (-not $process.WaitForExit(25000)) {
        $process.Kill()
        throw 'instp dejó la consola sin responder.'
    }
    $process.Refresh()
    $output = Get-Content -Raw -LiteralPath $stdoutFile
    $errorLog = Get-Content -Raw -LiteralPath $stderrFile
    if ($process.ExitCode -ne 0) {
        throw "instp devolvió $($process.ExitCode): $errorLog | $output"
    }
    foreach ($expected in @('SST_INSTP_STATUS_RETURNED', 'SST_INSTP_HELP_RETURNED',
                           'instp upgrade sst', 'instp check sst')) {
        if (-not $output.Contains($expected)) {
            throw "instp no mostró $expected. STDOUT: $output STDERR: $errorLog"
        }
    }
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\Cargo.toml')
    $match = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw 'Cargo.toml no contiene una versión canónica.' }
    $version = $match.Groups[1].Value.Trim()
    $reported = [regex]::Match($output, 'SST\s+(\d+\.\d+\.\d+-blue\.\d+)')
    if (-not $reported.Success -or $reported.Groups[1].Value -ne $version) {
        throw "instp sst informa versión incorrecta: esperada [$version], reportada [$($reported.Groups[1].Value)]"
    }
    Write-Host 'instp SST status/help: comprobación funcional superada en Windows.'
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
