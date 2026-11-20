param([Parameter(Mandatory=$true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$catalog = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\packages\index.json') | ConvertFrom-Json
if ($catalog.schema -ne 2) { throw 'SST: índice de paquetes debe utilizar schema 2' }
if (-not $catalog.packages -or @($catalog.packages).Count -eq 0) { throw 'SST: sin módulos instalables' }
foreach ($package in @($catalog.packages)) {
    if ($package.install.type -ne 'pkgx') { throw "SST: $($package.id) no utiliza .pkgx" }
    if ($package.execution.kind -in @('process', 'system')) { throw "SST: $($package.id) requiere proceso externo" }
    if ($package.execution.provider -eq 'wsl' -or $package.id -eq 'arch') {
        throw 'Arch WSL no forma parte de los paquetes SST'
    }
}
$composition = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\composition\mod.rs')
$service = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\application\packages.rs')
if ($composition -match 'WslEnvironmentBuiltin' -or $service -match 'install_wsl_environment|wsl\.exe') {
    throw 'La integración ejecutable de Arch/WSL sigue habilitada'
}

$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-native-packages-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$script = Join-Path $root 'native-packages.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
[IO.File]::WriteAllText($script, @'
instp info arch
instp search powershell
echo SST_NATIVE_PACKAGE_CHECK_DONE
'@, [Text.UTF8Encoding]::new($false))
try {
    $p = Start-Process -FilePath $exe -ArgumentList ('"' + $script + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if (-not $p.WaitForExit(15000)) {
        $p.Kill()
        throw 'El catálogo de paquetes bloqueó SST'
    }
    $p.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $out
    $stderr = Get-Content -Raw -LiteralPath $err
    if ($p.ExitCode -ne 0) { throw "El catálogo SST falló: $stderr | $stdout" }
    $combined = "$stdout`n$stderr"
    if (-not $combined.Contains('paquete no encontrado: arch') -or
        -not $stdout.Contains('powershell') -or
        -not $stdout.Contains('SST_NATIVE_PACKAGE_CHECK_DONE')) {
        throw "instp no cumple el catálogo nativo: $combined"
    }
    Write-Host 'SST: módulos PKGX, sin Arch/WSL ni ejecutables externos'
}
finally {
    Remove-Item -Force -Recurse -LiteralPath $root -ErrorAction SilentlyContinue
}
