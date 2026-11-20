param([Parameter(Mandatory = $true)][string]$SstPath)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$iconSource = Join-Path $PSScriptRoot '..\assets\sst-icon.ico'
if (-not (Test-Path -LiteralPath $iconSource -PathType Leaf)) {
    throw 'Falta el recurso ICO oficial de SST'
}
$header = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $iconSource).Path)
if ($header.Length -lt 22 -or [BitConverter]::ToUInt16($header, 0) -ne 0 -or
    [BitConverter]::ToUInt16($header, 2) -ne 1 -or
    [BitConverter]::ToUInt16($header, 4) -lt 1) {
    throw 'assets/sst-icon.ico no es un icono Windows válido'
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SstNativeIconResources
{
    [DllImport("shell32.dll", CharSet = CharSet.Unicode, EntryPoint = "ExtractIconExW")]
    public static extern uint ExtractIconEx(string file, int index, IntPtr large, IntPtr small, uint count);
}
'@
$count = [SstNativeIconResources]::ExtractIconEx($exe, -1, [IntPtr]::Zero, [IntPtr]::Zero, 0)
if ($count -lt 1) {
    throw "sst.exe carece de RT_GROUP_ICON: Windows muestra el icono predeterminado"
}
$gui = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\src\presentation\gui_slint.rs')
if (-not $gui.Contains('apply_embedded_sst_icon(hwnd)') -or
    -not $gui.Contains('WM_SETICON') -or
    -not $gui.Contains('LoadIconW')) {
    throw 'La GUI no asigna al HWND el icono oficial del recurso incrustado'
}
Write-Host "SST: icono Windows validado; $count recurso(s) nativos disponibles"
