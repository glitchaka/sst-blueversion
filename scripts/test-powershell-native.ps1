param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-native-powershell-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$exe = Join-Path $root 'sst.exe'
Copy-Item -LiteralPath $source -Destination $exe
$stdoutFile = Join-Path $root 'stdout.txt'
$stderrFile = Join-Path $root 'stderr.txt'

function Run-SstScript([string]$Text) {
    $scriptPath = Join-Path $root 'commands.sh'
    [IO.File]::WriteAllText($scriptPath, $Text, [Text.UTF8Encoding]::new($false))
    Remove-Item -LiteralPath $stdoutFile, $stderrFile -Force -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $exe -ArgumentList ('"' + $scriptPath + '"') -PassThru -RedirectStandardOutput $stdoutFile -RedirectStandardError $stderrFile
    if (-not $p.WaitForExit(45000)) {
        $p.Kill()
        throw 'El módulo nativo PowerShell bloqueó SST.'
    }
    $p.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $stdoutFile
    $stderr = Get-Content -Raw -LiteralPath $stderrFile
    if ($p.ExitCode -ne 0) {
        throw "SST devolvió $($p.ExitCode): $stderr | $stdout"
    }
    return $stdout
}

try {
    $first = Run-SstScript @'
instp install powershell
instp list
'@
    if ($first -notmatch 'Módulo PowerShell nativo SST') {
        throw "El instalador no registró el módulo propio: $first"
    }
    $second = Run-SstScript @'
psmodule
Get-Date
Get-ComputerInfo
Get-NetAdapter
Get-Disk
Test-Connection -TargetName 127.0.0.1
echo SST_NATIVE_POWERSHELL_COMPLETE
'@
    foreach ($expected in @(
        'Módulo PowerShell nativo SST',
        'SST_NATIVE_POWERSHELL_COMPLETE',
        'PING 127.0.0.1'
    )) {
        if (-not $second.Contains($expected)) {
            throw "No se ejecutó $expected. Salida: $second"
        }
    }
    $runtime = Join-Path $root 'packages\runtimes\powershell'
    if (Test-Path -LiteralPath (Join-Path $runtime 'pwsh.exe')) {
        throw 'El módulo de SST instaló indebidamente un PowerShell externo.'
    }
    Write-Host 'Módulo PowerShell nativo SST: instalación y cmdlets verificados.'
} finally {
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
