param(
    [Parameter(Mandatory = $true)][string]$SstPath,
    [Parameter(Mandatory = $true)][string]$PluginDirectory
)
$ErrorActionPreference = 'Stop'
$source = (Resolve-Path -LiteralPath $SstPath).Path
$plugin = (Resolve-Path -LiteralPath $PluginDirectory).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-powerShell-real-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$exe = Join-Path $root 'sst.exe'
Copy-Item -LiteralPath $source -Destination $exe
$installedDir = Join-Path $root 'packages/modules/powershell'
New-Item -ItemType Directory -Force -Path $installedDir | Out-Null
Copy-Item -Path (Join-Path $plugin '*') -Destination $installedDir -Recurse -Force
$journal = @{
    packages = @(@{
        id = 'powershell'
        name = 'Motor PowerShell SDK para SST'
        version = '7.5.13'
        category = 'module'
        portability = 'isolated'
        install_dir = 'packages\modules\powershell'
        commands = @()
        path_entries = @()
        persistent = $true
        functional_generation = ''
        execution = @{
            kind = 'embedded'
            entry = 'Sst.PowerShell.Bridge.dll'
            commands = @('psmodule','ps','powershell')
            extensions = @('.ps1')
            abi = 'sst-hostfxr/1'
            startup = 'load'
        }
    })
}
[IO.File]::WriteAllText(
    (Join-Path $root 'packages/installed.json'),
    ($journal | ConvertTo-Json -Depth 12),
    [Text.UTF8Encoding]::new($false)
)
$stdoutFile = Join-Path $root 'stdout.txt'
$stderrFile = Join-Path $root 'stderr.txt'
function Run-SstScript([string]$Text) {
    $scriptPath = Join-Path $root 'commands.sh'
    [IO.File]::WriteAllText($scriptPath, $Text, [Text.UTF8Encoding]::new($false))
    Remove-Item -LiteralPath $stdoutFile, $stderrFile -Force -ErrorAction SilentlyContinue
    $p = Start-Process -FilePath $exe -ArgumentList ('"' + $scriptPath + '"') -WorkingDirectory $root -PassThru -RedirectStandardOutput $stdoutFile -RedirectStandardError $stderrFile
    if (-not $p.WaitForExit(45000)) {
        $p.Kill()
        throw 'El runspace PowerShell dentro de SST bloqueó la consola.'
    }
    $p.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $stdoutFile
    $stderr = Get-Content -Raw -LiteralPath $stderrFile
    if ($p.ExitCode -ne 0) { throw "SST devolvió $($p.ExitCode): $stderr | $stdout" }
    return $stdout
}

try {
    $ps1 = Join-Path $root 'sdk-test.ps1'
    [IO.File]::WriteAllText($ps1, @'
param([int]$Count = 2)
$result = 1..$Count | ForEach-Object { $_ * 3 } | Measure-Object -Sum
"PS1_PIPELINE_TOTAL=$($result.Sum)"
'@, [Text.UTF8Encoding]::new($false))

    $first = Run-SstScript @'
instp list
psmodule status
psh -Command "'PS_SDK_EXPRESSION='+ (5 * 7)"
psh -File sdk-test.ps1 -Count 4
echo SST_POWERSHELL_FINISHED
'@
    foreach ($expected in @(
        'Motor PowerShell SDK para SST',
        'SST PowerShell SDK',
        'PS_SDK_EXPRESSION=35',
        'PS1_PIPELINE_TOTAL=30',
        'SST_POWERSHELL_FINISHED'
    )) {
        if (-not $first.Contains($expected)) {
            throw "PowerShell embebido no ejecutó '$expected'. Salida: $first"
        }
    }

    $direct = Run-SstScript @'
sdk-test.ps1 -Count 3
echo SST_DIRECT_PS1_FINISHED
'@
    if (-not $direct.Contains('PS1_PIPELINE_TOTAL=18') -or
        -not $direct.Contains('SST_DIRECT_PS1_FINISHED')) {
        throw "La extensión .ps1 no se entregó al motor PowerShell: $direct"
    }

    foreach ($forbidden in @('pwsh.exe','powershell.exe')) {
        if (Test-Path -LiteralPath (Join-Path $installedDir $forbidden)) {
            throw "El plugin contiene ejecutable prohibido: $forbidden"
        }
    }
    Write-Host 'PowerShell SDK embebido: cmdlets, expresiones, tuberías y .ps1 verificados.'
}
finally {
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
