param(
    [string]$Exe = '.\target\release\sst.exe',
    [int]$WarmupSeconds = 8,
    [int]$SampleSeconds = 10
)

$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Exe).Path
$outDir = Join-Path $PWD 'qa-results'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$process = Start-Process -FilePath $exePath -ArgumentList '--gui' -PassThru
try {
    Start-Sleep -Seconds $WarmupSeconds
    $process.Refresh()
    if ($process.HasExited) {
        throw "SST GUI terminó durante el warmup con código $($process.ExitCode)."
    }

    $cpuStart = $process.TotalProcessorTime.TotalSeconds
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $samples = @()

    for ($i = 0; $i -lt $SampleSeconds; $i++) {
        Start-Sleep -Seconds 1
        $process.Refresh()
        if ($process.HasExited) {
            throw "SST GUI terminó durante la medición con código $($process.ExitCode)."
        }
        $samples += [pscustomobject]@{
            second = $i + 1
            working_set_mb = [math]::Round($process.WorkingSet64 / 1MB, 2)
            private_mb = [math]::Round($process.PrivateMemorySize64 / 1MB, 2)
            cpu_seconds = [math]::Round($process.TotalProcessorTime.TotalSeconds, 4)
        }
    }

    $watch.Stop()
    $process.Refresh()
    $cpuDelta = $process.TotalProcessorTime.TotalSeconds - $cpuStart
    $elapsed = [math]::Max($watch.Elapsed.TotalSeconds, 0.001)
    $oneCorePercent = ($cpuDelta / $elapsed) * 100.0
    $machinePercent = $oneCorePercent / [Environment]::ProcessorCount
    $peakWorking = ($samples | Measure-Object -Property working_set_mb -Maximum).Maximum
    $avgWorking = ($samples | Measure-Object -Property working_set_mb -Average).Average
    $peakPrivate = ($samples | Measure-Object -Property private_mb -Maximum).Maximum

    $report = [ordered]@{
        executable = $exePath
        warmup_seconds = $WarmupSeconds
        sample_seconds = $SampleSeconds
        logical_processors = [Environment]::ProcessorCount
        cpu_seconds = [math]::Round($cpuDelta, 4)
        cpu_percent_one_core = [math]::Round($oneCorePercent, 3)
        cpu_percent_machine = [math]::Round($machinePercent, 3)
        avg_working_set_mb = [math]::Round($avgWorking, 2)
        peak_working_set_mb = [math]::Round($peakWorking, 2)
        peak_private_mb = [math]::Round($peakPrivate, 2)
        samples = $samples
    }

    $json = $report | ConvertTo-Json -Depth 5
    $json | Set-Content -LiteralPath (Join-Path $outDir 'gui-idle-resources.json') -Encoding UTF8
    Write-Host $json

    # Catch gross regressions while keeping the probe portable across GitHub-hosted runners.
    if ($oneCorePercent -gt 15.0) {
        throw "CPU idle excesiva: $([math]::Round($oneCorePercent,2))% de un núcleo."
    }
    if ($peakWorking -gt 300.0) {
        throw "Working set idle excesivo: $([math]::Round($peakWorking,2)) MB."
    }
}
finally {
    if ($process -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        $process.WaitForExit(5000) | Out-Null
    }
}
