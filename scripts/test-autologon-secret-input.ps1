param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-autologon-security-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$fixture = Join-Path $root 'command.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
$sentinel = 'SST_FAKE_PASSWORD_0Q5V_NOT_REAL'

function Run-Check([string]$command, [int]$expectedCode) {
    [IO.File]::WriteAllText($fixture, ($command + [Environment]::NewLine), [Text.UTF8Encoding]::new($false))
    Remove-Item -Force -LiteralPath $out, $err -ErrorAction SilentlyContinue
    $process = Start-Process -FilePath $exe -ArgumentList ('"' + $fixture + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    try {
        if (-not $process.WaitForExit(12000)) {
            $process.Kill()
            throw "Autologon blocked while executing a safe negative test."
        }
        $process.Refresh()
        $stdout = Get-Content -Raw -LiteralPath $out
        $stderr = Get-Content -Raw -LiteralPath $err
        $combined = $stdout + $stderr
        if ($combined.Contains($sentinel)) {
            throw "A credential value appeared in the terminal output."
        }
        if ($process.ExitCode -ne $expectedCode) {
            throw "Unexpected exit code: $($process.ExitCode), expected $expectedCode"
        }
        return $combined
    }
    finally {
        $process.Refresh()
        if (-not $process.HasExited) { $process.Kill() }
    }
}

try {
    foreach ($command in @(
        "sys autologon enable -u operator -d example.invalid -p $sentinel",
        "autologon -u operator -d example.invalid --password $sentinel",
        "sys autologon -u operator -d example.invalid --password=$sentinel",
        "sudo sys autologon -u operator -d example.invalid -p $sentinel"
    )) {
        $output = Run-Check $command 2
        if ($output -notmatch 'contraseña en línea prohibida') {
            throw "The shell did not reject inline credentials."
        }
    }

    $help = Run-Check 'sys autologon --help' 0
    if ($help -notmatch 'ventana protegida' -or $help -notmatch 'autologon disable') {
        throw 'Autologon help does not describe the protected workflow.'
    }

    $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\support\autologon.rs')
    if ($source -notmatch 'LsaStorePrivateData' -or $source -notmatch 'request_password') {
        throw 'Autologon is not implemented with native protected credentials.'
    }
    if ($source -match 'run_elevated_cli|Sysinternals Autologon in mode CLI|Autologon64.exe') {
        throw 'The obsolete executable/command-line credential path remains.'
    }
    if ($source -match '(?s)struct ElevatedRequest\s*\{[^}]*\bpassword\s*:') {
        throw 'An elevated process would receive a plaintext password.'
    }
    Write-Host 'Autologon: inline credentials rejected; protected CredUI/LSA backend verified.'
}
finally {
    Remove-Item -Force -Recurse -LiteralPath $root -ErrorAction SilentlyContinue
}
