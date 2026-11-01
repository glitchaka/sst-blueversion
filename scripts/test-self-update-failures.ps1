param(
    [Parameter(Mandatory = $true)]
    [string]$SstPath
)

$ErrorActionPreference = 'Stop'

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SstQaWindows {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll", SetLastError=true)]
    public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
}
'@

$sst = [System.IO.Path]::GetFullPath($SstPath)
if (-not (Test-Path -LiteralPath $sst -PathType Leaf)) {
    throw "No existe SST en $sst"
}

$oldExe = Join-Path $env:SystemRoot 'System32\whoami.exe'
$newExe = Join-Path $env:SystemRoot 'System32\where.exe'
$oldHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $oldExe).Hash.ToLowerInvariant()
$newHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $newExe).Hash.ToLowerInvariant()
if ($oldHash -eq $newHash) { throw 'La prueba necesita ejecutables viejo/nuevo diferentes.' }

$names = @(
    'SST_INTERNAL_APPLY_UPDATE','SST_UPDATE_PARENT_PID','SST_UPDATE_TARGET',
    'SST_UPDATE_READY','SST_UPDATE_BACKUP','SST_UPDATE_PENDING',
    'SST_UPDATE_APPLIED_PENDING','SST_UPDATE_APPLIED','SST_UPDATE_STAGED',
    'SST_UPDATE_SHA256','SST_UPDATE_HELPER_CLEANUP'
)

function Clear-UpdateEnv {
    foreach ($name in $names) {
        Remove-Item "Env:$name" -ErrorAction SilentlyContinue
    }
}

function Close-ProcessDialogs([int]$ProcessId) {
    $script:closedDialog = $false
    $callback = [SstQaWindows+EnumWindowsProc]{
        param([IntPtr]$hWnd, [IntPtr]$lParam)
        [uint32]$ownerPid = 0
        [void][SstQaWindows]::GetWindowThreadProcessId($hWnd, [ref]$ownerPid)
        if ($ownerPid -eq [uint32]$ProcessId) {
            [void][SstQaWindows]::PostMessage($hWnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
            $script:closedDialog = $true
        }
        return $true
    }
    [void][SstQaWindows]::EnumWindows($callback, [IntPtr]::Zero)
    return $script:closedDialog
}

function New-Case([string]$Name) {
    $root = Join-Path $env:RUNNER_TEMP ("sst-update-failure-$Name-" + [guid]::NewGuid().ToString('N'))
    $updateDir = Join-Path $root 'update'
    $helperDir = Join-Path $updateDir 'tmp'
    New-Item -ItemType Directory -Force -Path $helperDir | Out-Null
    $case = [ordered]@{
        Root = $root
        UpdateDir = $updateDir
        Helper = Join-Path $helperDir 'sst-update-helper.exe'
        Target = Join-Path $root 'sst.exe'
        Ready = Join-Path $root 'sst.exe.new'
        Backup = Join-Path $root 'sst.exe.old'
        Pending = Join-Path $updateDir 'pending.json'
        AppliedPending = Join-Path $updateDir 'applied.pending.json'
        Applied = Join-Path $updateDir 'applied.json'
    }
    Copy-Item -LiteralPath $sst -Destination $case.Helper -Force
    Copy-Item -LiteralPath $oldExe -Destination $case.Target -Force
    Copy-Item -LiteralPath $newExe -Destination $case.Ready -Force
    '{"pending":true}' | Set-Content -LiteralPath $case.Pending -Encoding utf8NoBOM
    '{"applied":true}' | Set-Content -LiteralPath $case.AppliedPending -Encoding utf8NoBOM
    return [pscustomobject]$case
}

function Set-HelperEnv($Case, [string]$Hash) {
    $env:SST_INTERNAL_APPLY_UPDATE = '1'
    $env:SST_UPDATE_PARENT_PID = '4294967294'
    $env:SST_UPDATE_TARGET = $Case.Target
    $env:SST_UPDATE_READY = $Case.Ready
    $env:SST_UPDATE_BACKUP = $Case.Backup
    $env:SST_UPDATE_PENDING = $Case.Pending
    $env:SST_UPDATE_APPLIED_PENDING = $Case.AppliedPending
    $env:SST_UPDATE_APPLIED = $Case.Applied
    $env:SST_UPDATE_STAGED = $Case.Ready
    $env:SST_UPDATE_SHA256 = $Hash
}

function Invoke-Helper($Case, [int]$TimeoutMs = 20000) {
    $process = Start-Process -FilePath $Case.Helper -ArgumentList '--gui' -PassThru -WindowStyle Hidden
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    $sawDialog = $false
    while (-not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) {
        if (Close-ProcessDialogs $process.Id) { $sawDialog = $true }
        Start-Sleep -Milliseconds 100
        $process.Refresh()
    }
    if (-not $process.HasExited) {
        try { $process.Kill($true) } catch {}
        throw "Helper no terminó dentro de $TimeoutMs ms (dialog=$sawDialog)"
    }
    return $process.ExitCode
}

function Assert-TargetOld($Case, [string]$Message) {
    if (-not (Test-Path -LiteralPath $Case.Target -PathType Leaf)) { throw "${Message}: sst.exe desapareció" }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Case.Target).Hash.ToLowerInvariant()
    if ($hash -ne $oldHash) { throw "${Message}: sst.exe fue modificado ($hash)" }
}

function Remove-Case($Case) {
    Clear-UpdateEnv
    Start-Sleep -Milliseconds 200
    Remove-Item -Recurse -Force -LiteralPath $Case.Root -ErrorAction SilentlyContinue
}

$results = [System.Collections.Generic.List[string]]::new()
Clear-UpdateEnv

$case = New-Case 'bad-hash'
try {
    Set-HelperEnv $case ('0' * 64)
    $exit = Invoke-Helper $case 10000
    if ($exit -eq 0) { throw 'Hash incorrecto fue aceptado.' }
    Assert-TargetOld $case 'Hash incorrecto'
    if (-not (Test-Path -LiteralPath $case.Ready -PathType Leaf)) { throw 'Hash incorrecto consumió staging.' }
    $results.Add('PASS bad-hash: target intacto')
} finally { Remove-Case $case }

$case = New-Case 'missing-ready'
try {
    Remove-Item -Force -LiteralPath $case.Ready
    Set-HelperEnv $case $newHash
    $exit = Invoke-Helper $case 10000
    if ($exit -eq 0) { throw 'Staging ausente fue aceptado.' }
    Assert-TargetOld $case 'Staging ausente'
    $results.Add('PASS missing-ready: target intacto')
} finally { Remove-Case $case }

$case = New-Case 'bad-parent-pid'
try {
    Set-HelperEnv $case $newHash
    $env:SST_UPDATE_PARENT_PID = 'no-es-un-pid'
    $exit = Invoke-Helper $case 10000
    if ($exit -eq 0) { throw 'PID inválido fue aceptado.' }
    Assert-TargetOld $case 'PID inválido'
    $results.Add('PASS bad-parent-pid: target intacto')
} finally { Remove-Case $case }

$case = New-Case 'locked-target'
$lock = $null
try {
    Set-HelperEnv $case $newHash
    $lock = [System.IO.File]::Open($case.Target, [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    $exit = Invoke-Helper $case 20000
    if ($exit -eq 0) { throw 'ReplaceFileW reportó éxito con target bloqueado en exclusiva.' }
    # La prueba misma mantiene el archivo en exclusivo; soltar el lock antes de
    # inspeccionar SHA evita confundir ese bloqueo con un fallo del updater.
    $lock.Dispose()
    $lock = $null
    Assert-TargetOld $case 'Target bloqueado'
    if (-not (Test-Path -LiteralPath $case.Ready -PathType Leaf)) { throw 'Fallo de ReplaceFileW consumió staging.' }
    $results.Add('PASS locked-target: reintenta y conserva target')
} finally {
    if ($null -ne $lock) { $lock.Dispose() }
    Remove-Case $case
}

$case = New-Case 'missing-receipt'
try {
    Remove-Item -Force -LiteralPath $case.AppliedPending
    Set-HelperEnv $case $newHash
    $exit = Invoke-Helper $case 15000
    if ($exit -eq 0) { throw 'La prueba esperaba fallo de recibo y el helper no lo reportó.' }
    if (-not (Test-Path -LiteralPath $case.Target -PathType Leaf)) { throw 'Fallo de recibo dejó la instalación sin sst.exe.' }
    $installed = (Get-FileHash -Algorithm SHA256 -LiteralPath $case.Target).Hash.ToLowerInvariant()
    if ($installed -ne $newHash) { throw "Fallo de recibo dejó target inesperado: $installed" }
    if (-not (Test-Path -LiteralPath $case.Backup -PathType Leaf)) { throw 'Fallo de recibo perdió rollback.' }
    $rollback = (Get-FileHash -Algorithm SHA256 -LiteralPath $case.Backup).Hash.ToLowerInvariant()
    if ($rollback -ne $oldHash) { throw 'Fallo de recibo dejó rollback incorrecto.' }
    $results.Add('PASS missing-receipt: target nuevo y rollback conservado (helper reporta error)')
} finally { Remove-Case $case }

$case = New-Case 'missing-env'
try {
    Set-HelperEnv $case $newHash
    Remove-Item Env:SST_UPDATE_SHA256
    $exit = Invoke-Helper $case 10000
    if ($exit -eq 0) { throw 'Helper aceptó variable crítica ausente.' }
    Assert-TargetOld $case 'Variable crítica ausente'
    $results.Add('PASS missing-env: target intacto')
} finally { Remove-Case $case }

Write-Host ''
Write-Host 'Self-update failure matrix:'
$results | ForEach-Object { Write-Host "  $_" }
Write-Host "Failure cases OK: $($results.Count)"
