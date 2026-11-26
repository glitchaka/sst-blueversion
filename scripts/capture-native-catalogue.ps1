param([string]$Exe = '.\target\release\sst.exe', [string]$Output = '.\qa-visual')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SSTWindowCapture {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int height,bool repaint);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h,IntPtr dc,uint flags);
}
'@
$exePath=(Resolve-Path -LiteralPath $Exe).Path
New-Item -ItemType Directory -Force -Path $Output | Out-Null
$outputPath=(Resolve-Path -LiteralPath $Output).Path
$process=$null
$shell=New-Object -ComObject WScript.Shell
function Capture([string]$Name) {
    $process.Refresh()
    if($process.HasExited){throw "SST exited before $Name"}
    $hwnd=$process.MainWindowHandle
    if($hwnd -eq [IntPtr]::Zero){throw "SST has no window for $Name"}
    $bitmap=New-Object System.Drawing.Bitmap 1100,760
    $graphics=[System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $dc=$graphics.GetHdc()
        try { if(-not [SSTWindowCapture]::PrintWindow($hwnd,$dc,2)){throw "PrintWindow failed: $Name"} }
        finally {$graphics.ReleaseHdc($dc)}
        $colors=New-Object 'System.Collections.Generic.HashSet[int]'
        for($y=0;$y -lt $bitmap.Height;$y+=3) {
            for($x=0;$x -lt $bitmap.Width;$x+=3) { [void]$colors.Add($bitmap.GetPixel($x,$y).ToArgb()) }
        }
        if($colors.Count -lt 20){throw "Blank or incomplete capture: $Name ($($colors.Count) colors)"}
        $bitmap.Save((Join-Path $outputPath "$Name.png"),[System.Drawing.Imaging.ImageFormat]::Png)
        Write-Host "CAPTURE $Name $($colors.Count) sampled colors"
    } finally {$graphics.Dispose();$bitmap.Dispose()}
}
function Command([string]$Text,[int]$Pause=2) {
    if(-not $shell.AppActivate($process.Id)){throw 'Could not focus SST'}
    $shell.SendKeys($Text)
    $shell.SendKeys('{ENTER}')
    Start-Sleep -Seconds $Pause
}
try {
    $process=Start-Process -FilePath $exePath -ArgumentList '--gui' -PassThru
    $deadline=[DateTime]::UtcNow.AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        if($process.HasExited){throw 'SST exited during startup'}
    } until($process.MainWindowHandle -ne [IntPtr]::Zero -or [DateTime]::UtcNow -gt $deadline)
    if($process.MainWindowHandle -eq [IntPtr]::Zero){throw 'SST window unavailable'}
    [void][SSTWindowCapture]::MoveWindow($process.MainWindowHandle,0,0,1100,760,$true)
    [void][SSTWindowCapture]::SetForegroundWindow($process.MainWindowHandle)
    Start-Sleep -Seconds 3
    Command 'clear'
    Command 'sys'
    Capture '01-reference'
    Command 'clear'
    Command 'ping 127.0.0.1' 6
    Capture '02-live-ping'
    Command 'clear'
    Command 'sys info'
    Capture '03-system'
    Command 'clear'
    Command 'net routes'
    Capture '04-routes'
    Command 'clear'
    if(-not $shell.AppActivate($process.Id)){throw 'Could not focus SST'}
    $shell.SendKeys('sys ')
    $shell.SendKeys('{TAB}')
    Start-Sleep -Seconds 2
    Capture '05-completion'
    $shell.SendKeys('{ESC}')
    $shell.SendKeys('^c')
    Command 'config palette aqua'
    Command 'clear'
    Command 'sys'
    Capture '06-aqua'
} finally {
    if($process -and -not $process.HasExited){Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue}
}
