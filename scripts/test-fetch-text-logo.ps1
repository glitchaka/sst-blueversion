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
    if ($initial.Contains('%%%') -or -not $initial.Contains('OS:')) {
        throw "fetch --startup debe mostrar datos del sistema sin desplazar el logo: $initial"
    }
    $narrowEnd = $stdout.IndexOf('SST_NARROW_FETCH_DONE')
    if ($narrowEnd -lt 0 -or $narrowEnd -le $separator) {
        throw "fetch para GUI estrecha no terminó: $stdout"
    }
    $narrow = $stdout.Substring($separator, $narrowEnd - $separator)
    if ($narrow -notmatch '%{3,}' -or -not $narrow.Contains('OS:') -or
        -not $narrow.Contains('RAM:')) {
        throw "El arranque estrecho no muestra logo SST y datos juntos: $narrow"
    }
    $wideEnd = $stdout.IndexOf('SST_WIDE_FETCH_DONE')
    if ($wideEnd -lt 0 -or $wideEnd -le $narrowEnd) {
        throw "fetch para GUI ancha no terminó: $stdout"
    }
    $wide = $stdout.Substring($narrowEnd + 'SST_NARROW_FETCH_DONE'.Length,
        $wideEnd - $narrowEnd - 'SST_NARROW_FETCH_DONE'.Length)
    if ($wide -notmatch '%{3,}' -or -not $wide.Contains('OS:') -or
        -not $wide.Contains('RAM:')) {
        throw "SST no dibuja logo y datos en arranque de ventana ancha: $wide"
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
    if (-not $gui.Contains('startup_logo_image()') -or
        -not $gui.Contains('set_startup_logo_image') -or
        -not $gui.Contains('startup-logo-visible')) {
        throw 'El logo SST no está integrado como imagen nativa independiente del scroll.'
    }
    $bootstrap = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\adapters\terminal\embedded.rs')
    if (-not $bootstrap.Contains('fetch --startup-cols={cols}') -or
        $bootstrap.Contains('"fetch --startup".to_owned()')) {
        throw 'SST GUI no imprime el logo ASCII de respaldo en todas las anchuras.'
    }
    if (-not $gui.Contains('key_is(text.as_str(), slint::platform::Key::Return)')) {
        throw 'El logotipo de arranque se oculta antes de enviar un comando.'
    }
    Write-Host 'SST startup: logo ASCII compacto en todas las anchuras, original de 37 líneas y overlay hasta Enter.'
}
finally {
    if ($process) {
        $process.Refresh()
        if (-not $process.HasExited) { $process.Kill() }
    }
    Remove-Item -Recurse -Force -LiteralPath $root -ErrorAction SilentlyContinue
}
