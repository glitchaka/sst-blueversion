param([Parameter(Mandatory=$true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$sst = [IO.Path]::GetFullPath($SstPath)
if (-not (Test-Path -LiteralPath $sst -PathType Leaf)) { throw "SST no existe: $sst" }
$root = Join-Path $env:RUNNER_TEMP ("sst-process-sub-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $root | Out-Null
try {
    $script = Join-Path $root 'read-pipe.sh'
    $stdout = Join-Path $root 'stdout.txt'
    $stderr = Join-Path $root 'stderr.txt'
    $content = @'
#!/usr/bin/env bash
while IFS= read -r line; do
    printf '<%s>\n' "$line"
done < <(printf 'uno\ndos\n')
'@
    [IO.File]::WriteAllText($script, $content, [Text.UTF8Encoding]::new($false))
    $proc = Start-Process -FilePath $sst -ArgumentList @($script) -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    if (-not $proc.WaitForExit(20000)) {
        $proc.Kill()
        throw 'El bucle while/read quedó bloqueado (20 segundos).'
    }
    $output = Get-Content -Raw -LiteralPath $stdout
    $errors = Get-Content -Raw -LiteralPath $stderr
    if ($proc.ExitCode -ne 0) {
        throw "Nwash devolvió $($proc.ExitCode): $errors"
    }
    if ($output -notmatch '(?s)<uno>\r?\n<dos>') {
        throw "No se recuperaron todas las líneas de la tubería: $output"
    }
    if ($errors -match '233|canalización|Broken pipe|PIPE_NOT_CONNECTED') {
        throw "Falló el cierre de tubería Windows: $errors"
    }
    Write-Host 'Nwash Windows process substitution + while/read aprobados.'
}
finally {
    Remove-Item -LiteralPath $root -Force -Recurse -ErrorAction SilentlyContinue
}
