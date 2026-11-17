param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-fetch-text-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$fixture = Join-Path $root 'fetch.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
$script = "fetch --plain`nfetch --full --plain`necho SST_FETCH_COMPLETE`n"
[IO.File]::WriteAllText($fixture, $script, [Text.UTF8Encoding]::new($false))
$process = $null
try {
    $process = Start-Process -FilePath $exe -ArgumentList ('"' + $fixture + '"') -PassThru -RedirectStandardOutput $out -RedirectStandardError $err
    if (-not $process.WaitForExit(18000)) {
        $process.Kill()
        throw 'fetch bloqueó SST.'
    }
    $process.Refresh()
    $stdout = Get-Content -Raw -LiteralPath $out
    $stderr = Get-Content -Raw -LiteralPath $err
    if ($process.ExitCode -ne 0) { throw "fetch falló: $stderr" }
    if ($stdout -notmatch '(?m)^SST\s{3}\S+@' -or $stdout -notmatch '(?m)^SST · Shell Shock Tool\s{3}\S+@') {
        throw "fetch no imprimió SST como texto compacto: $stdout"
    }
    if ($stdout -match '[█▀▄]' -or $stdout -match [char]27) {
        throw 'fetch --plain imprimió pixel art o códigos ANSI.'
    }
    if (-not $stdout.Contains('SST_FETCH_COMPLETE')) {
        throw 'fetch no devolvió el control al intérprete.'
    }
    $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\application\system.rs')
    if ($source.Contains('FETCH_LOGO_PIXELS') -or $source.Contains('render_fetch_logo(')) {
        throw 'El antiguo logo SST dibujado sigue en el código.'
    }
    Write-Host 'fetch/neofetch: SST es texto nativo, compacto y sin arte rasterizado.'
}
finally {
    if ($process) {
        $process.Refresh()
        if (-not $process.HasExited) { $process.Kill() }
    }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
