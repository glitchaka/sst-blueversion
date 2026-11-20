param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$root = Join-Path $env:RUNNER_TEMP ('sst-fetch-text-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$fixture = Join-Path $root 'fetch.sh'
$out = Join-Path $root 'stdout.txt'
$err = Join-Path $root 'stderr.txt'
$script = "fetch --startup`necho SST_STARTUP_FETCH_DONE`nfetch --startup-cols=58`necho SST_NARROW_FETCH_DONE`nfetch --startup-cols=100`necho SST_WIDE_FETCH_DONE`nfetch --plain`nfetch --full --plain`necho SST_FETCH_COMPLETE`n"
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
    $separator = $stdout.IndexOf('SST_STARTUP_FETCH_DONE')
    if ($separator -lt 0) {
        throw "fetch --startup no devolvió control a la consola: $stdout"
    }
    $initial = $stdout.Substring(0, $separator)
    if (-not $initial.Contains('OS:') -or -not $initial.Contains('RAM:')) {
        throw "fetch --startup no presenta metadatos del sistema: $initial"
    }
    $narrowEnd = $stdout.IndexOf('SST_NARROW_FETCH_DONE')
    if ($narrowEnd -lt 0 -or $narrowEnd -le $separator) {
        throw "fetch para ventana estrecha no terminó: $stdout"
    }
    $narrow = $stdout.Substring($separator, $narrowEnd - $separator)
    if (-not $narrow.Contains('OS:') -or -not $narrow.Contains('RAM:')) {
        throw "fetch estrecho no muestra los datos del sistema: $narrow"
    }
    $wideEnd = $stdout.IndexOf('SST_WIDE_FETCH_DONE')
    if ($wideEnd -lt 0 -or $wideEnd -le $narrowEnd) {
        throw "fetch para ventana ancha no terminó: $stdout"
    }
    $wide = $stdout.Substring($narrowEnd + 'SST_NARROW_FETCH_DONE'.Length,
        $wideEnd - $narrowEnd - 'SST_NARROW_FETCH_DONE'.Length)
    if (-not $wide.Contains('OS:') -or -not $wide.Contains('RAM:')) {
        throw "fetch ancho no muestra los datos del sistema: $wide"
    }
    if ($stdout -notmatch '(?m)^   %{20,}' -or $stdout -notmatch '(?m)^%%.*?%{4,}') {
        throw "fetch no imprimió el logotipo ASCII SST: $stdout"
    }
    # The interactive compact fetch intentionally uses ANSI colors.
    # Only inspect the two subsequent --plain renders for color leakage.
    $plainOutput = $stdout.Substring($wideEnd + 'SST_WIDE_FETCH_DONE'.Length)
    if ($plainOutput -match [char]27) {
        throw 'fetch --plain imprimió códigos ANSI.'
    }
    if (-not $stdout.Contains('SST_FETCH_COMPLETE')) {
        throw 'fetch no devolvió el control al intérprete.'
    }
    $source = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\application\system.rs')
    if (-not $source.Contains('FETCH_LOGO_ASCII')) {
        throw 'No se encontró el nuevo logotipo ASCII SST en el código.'
    }
    $gui = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\presentation\gui_slint.rs')
    if ($gui.Contains('startup-logo-visible') -or $gui.Contains('set_startup_logo_image')) {
        throw 'El overlay SST obsoleto ha reaparecido en la GUI.'
    }
    $bootstrap = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\adapters\terminal\embedded.rs')
    if (-not $bootstrap.Contains('fetch --startup-cols={cols}')) {
        throw 'La GUI no integra fetch en el arranque.'
    }
    Write-Host 'SST startup: metadatos, fetch en todas las anchuras y ningún overlay flotante.'
}
finally {
    if ($process) {
        $process.Refresh()
        if (-not $process.HasExited) { $process.Kill() }
    }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
