param(
    [Parameter(Mandatory=$true)][string]$SstPath,
    [Parameter(Mandatory=$true)][string]$PluginDirectory
)
$ErrorActionPreference='Stop'
$root = Join-Path $env:RUNNER_TEMP ('sst-powershell-plugin-' + [guid]::NewGuid().ToString('N'))
$plugin = Join-Path $root 'packages\modules\powershell'
New-Item -ItemType Directory -Force -Path $plugin | Out-Null
Copy-Item -LiteralPath (Resolve-Path $SstPath).Path -Destination (Join-Path $root 'sst.exe')
Copy-Item -Path (Join-Path (Resolve-Path $PluginDirectory).Path '*') -Destination $plugin -Recurse -Force
$state = @{
    packages = @(@{
        id='powershell'; name='PowerShell SDK'; version='self-test'
        category='module'; portability='isolated'
        install_dir='packages\modules\powershell'; commands=@(); path_entries=@()
        persistent=$true; functional_generation='test'
        execution=@{
            kind='embedded'; entry='Sst.PowerShell.Bridge.dll'
            commands=@('psmodule','powershell','ps'); extensions=@('.ps1')
            abi='sst-hostfxr/1'; startup='load'
        }
    })
}
$statePath = Join-Path $root 'packages\installed.json'
[IO.File]::WriteAllText($statePath, ($state | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
$ps1 = Join-Path $root 'example.ps1'
[IO.File]::WriteAllText($ps1, @'
param([string]$Name)
$numbers = 1..4 | Where-Object { $_ % 2 -eq 0 }
"PS1_OK_$($Name)_$($numbers.Count)"
'@, [Text.UTF8Encoding]::new($false))
$sh = Join-Path $root 'run.sh'
[IO.File]::WriteAllText($sh, @'
psmodule status
psmodule -Command '1..3 | Measure-Object | Select-Object -ExpandProperty Count'
./example.ps1 PLUGIN
echo SST_POWERSHELL_DONE
'@, [Text.UTF8Encoding]::new($false))
function Run-SST([string]$target, [string]$argsText) {
    $stdoutFile = Join-Path $root 'stdout.txt'
    $stderrFile = Join-Path $root 'stderr.txt'
    Remove-Item -Force $stdoutFile,$stderrFile -ErrorAction SilentlyContinue
    $exe = Join-Path $root 'sst.exe'
    $process = Start-Process -FilePath $exe -ArgumentList $argsText -WorkingDirectory $root -PassThru -RedirectStandardOutput $stdoutFile -RedirectStandardError $stderrFile
    if (-not $process.WaitForExit(45000)) {
        $process.Kill()
        throw "PowerShell SDK no terminó el escenario $target"
    }
    $process.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $stdoutFile
    $stderr = Get-Content -Raw -LiteralPath $stderrFile
    if ($process.ExitCode -ne 0) {
        throw "PowerShell SDK $target terminó con $($process.ExitCode): $stderr | $stdout"
    }
    return $stdout
}
try {
    $result = Run-SST 'Nwash' ('"' + $sh + '"')
    foreach ($expected in @('7.5.', '3', 'PS1_OK_PLUGIN_2', 'SST_POWERSHELL_DONE')) {
        if (-not $result.Contains($expected)) {
            throw "No se verificó $expected en el motor PowerShell: $result"
        }
    }
    $direct = Run-SST 'script .ps1' ('"' + $ps1 + '" DIRECT')
    if (-not $direct.Contains('PS1_OK_DIRECT_2')) {
        throw "SST no ejecutó el archivo PS1 directo: $direct"
    }
    Write-Host 'SST PowerShell SDK: runspace real, tuberías, variables y .ps1 desde Nwash/CLI.'
}
finally {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
}
