param(
    [Parameter(Mandatory=$true)][string]$SstPath,
    [Parameter(Mandatory=$true)][string]$PluginDirectory
)
$ErrorActionPreference='Stop'
$root = Join-Path $env:RUNNER_TEMP ('sst-powershell-plugin-' + [guid]::NewGuid().ToString('N'))
$plugin = Join-Path $root 'packages\win\modules\powershell'
New-Item -ItemType Directory -Force -Path $root | Out-Null
Copy-Item -LiteralPath (Resolve-Path $SstPath).Path -Destination (Join-Path $root 'sst.exe')
$sourceDirectory = (Resolve-Path -LiteralPath $PluginDirectory).Path
$archive = Join-Path $root 'powershell.pkgx'
& python (Join-Path $PSScriptRoot 'pack-pkgx.py') --id powershell --version 7.5.13 --target win --kind embedded --source $sourceDirectory --output $archive --exclude-dotnet-apphost --require-file Sst.PowerShell.Bridge.dll --require-file hostfxr.dll --require-file Sst.PowerShell.Bridge.runtimeconfig.json
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $archive -PathType Leaf)) {
    throw 'El empaquetador nativo SST-PACKAGE/1 no produjo un módulo válido'
}
$installScript = Join-Path $root 'install.sh'
[IO.File]::WriteAllText($installScript, "instp install powershell.pkgx -p`necho SST_PLUGIN_INSTALL_DONE`n", [Text.UTF8Encoding]::new($false))
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
psmodule -File example.ps1 MODULE
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
    return ($stdout + "`n--- STDERR SST $target ---`n" + $stderr)
}
try {
    $installed = Run-SST 'instp install local .pkgx' ('"' + $installScript + '"')
    if (-not $installed.Contains('Instalado:') -or -not $installed.Contains('SST_PLUGIN_INSTALL_DONE')) {
        throw "instp no instaló el archivo SST-PACKAGE/1: $installed"
    }
    foreach ($required in @('Sst.PowerShell.Bridge.dll', 'hostfxr.dll', 'Sst.PowerShell.Bridge.runtimeconfig.json')) {
        if (-not (Test-Path -LiteralPath (Join-Path $plugin $required) -PathType Leaf)) {
            throw "instp no desplegó $required en packages/win/modules/powershell"
        }
    }
    $unexpected = @(Get-ChildItem -LiteralPath $plugin -Recurse -File | Where-Object { $_.Extension -ieq '.exe' })
    if ($unexpected.Count -gt 0) {
        throw "instp instaló un ejecutable adicional: $($unexpected[0].FullName)"
    }
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
