param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-arch-package-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$script = Join-Path $root 'arch-test.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
[IO.File]::WriteAllText($script, @'
instp info arch
instp search arch
echo SST_ARCH_CATALOG_DONE
'@, [Text.UTF8Encoding]::new($false))
try {
    $p = Start-Process -FilePath $exe -ArgumentList ('"' + $script + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if (-not $p.WaitForExit(15000)) {
        $p.Kill()
        throw 'El catálogo Arch bloqueó SST.'
    }
    $p.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $out
    $stderr = Get-Content -Raw -LiteralPath $err
    if ($p.ExitCode -ne 0) { throw "instp info arch falló: $stderr | $stdout" }
    foreach ($expected in @('Arch Linux', 'SST-PACKAGE/1', 'environment', 'WSL2', 'SST_ARCH_CATALOG_DONE')) {
        if (-not $stdout.Contains($expected)) {
            throw "Arch no declaró su adaptador en instp: $expected / $stdout"
        }
    }
    $catalog = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\packages\index.json') | ConvertFrom-Json
    if ($catalog.schema -ne 2) { throw 'El catálogo debe utilizar schema 2.' }
    $arch = @($catalog.packages | Where-Object { $_.id -eq 'arch' })[0]
    if ($arch.execution.kind -ne 'environment' -or $arch.execution.target -ne 'unix' -or $arch.execution.provider -ne 'wsl' -or $arch.execution.distribution -ne 'archlinux') {
        throw 'El entorno Arch no cumple SST-PACKAGE/1.'
    }
    Write-Host 'Arch: manifiesto y catálogo de WSL2 válidos, sin modificar Windows.'
}
finally {
    Remove-Item -Force -Recurse -LiteralPath $root -ErrorAction SilentlyContinue
}
