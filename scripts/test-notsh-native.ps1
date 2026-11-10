param(
    [Parameter(Mandatory = $true)]
    [string] $SstPath
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $SstPath).Path
$dir = Join-Path $env:RUNNER_TEMP ("sst-notsh-native-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$fixture = Join-Path $dir 'native-notsh.sh'
$stdout = Join-Path $dir 'notsh.out'
$stderr = Join-Path $dir 'notsh.err'
$commands = @'
notsh lan show interfaces
echo SST_NOTSH_LAN_RETURNED
notsh wlan show interfaces
echo SST_NOTSH_WLAN_RETURNED
notsh interface ipv4 show interfaces
echo SST_NOTSH_IPV4_RETURNED
notsh interface ipv6 show interfaces
echo SST_NOTSH_IPV6_RETURNED
notsh winhttp show proxy
echo SST_NOTSH_WINHTTP_RETURNED
notsh winsock show catalog
echo SST_NOTSH_WINSOCK_RETURNED
notsh advfirewall show allprofiles
echo SST_NOTSH_FIREWALL_RETURNED
notsh http show iplisten
echo SST_NOTSH_HTTP_IP_RETURNED
notsh http show urlacl
echo SST_NOTSH_HTTP_URL_RETURNED
'@
[IO.File]::WriteAllText($fixture, $commands, [Text.UTF8Encoding]::new($false))
$process = Start-Process -FilePath $exe -ArgumentList ('"' + $fixture + '"') -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
try {
    if (-not $process.WaitForExit(45000)) {
        $process.Kill()
        throw 'NOTSH bloqueó la consola durante una consulta nativa.'
    }
    $process.Refresh()
    $result = Get-Content -LiteralPath $stdout -Raw
    $errors = Get-Content -LiteralPath $stderr -Raw
    if ($process.ExitCode -ne 0) {
        throw "NOTSH terminó de forma anormal ($($process.ExitCode)): $errors"
    }
    foreach ($marker in @(
        'SST_NOTSH_LAN_RETURNED', 'SST_NOTSH_WLAN_RETURNED',
        'SST_NOTSH_IPV4_RETURNED', 'SST_NOTSH_IPV6_RETURNED',
        'SST_NOTSH_WINHTTP_RETURNED', 'SST_NOTSH_WINSOCK_RETURNED',
        'SST_NOTSH_FIREWALL_RETURNED',
        'SST_NOTSH_HTTP_IP_RETURNED', 'SST_NOTSH_HTTP_URL_RETURNED'
    )) {
        if (-not $result.Contains($marker)) {
            throw "No regresó de $marker. STDOUT: $result; STDERR: $errors"
        }
    }
    if ($result -notmatch 'LAN|Dot3|Ethernet') { throw "Diagnóstico LAN sin contexto: $result" }
    if ($result -notmatch 'WLAN|Wi-Fi|WiFi') { throw "Diagnóstico WLAN sin contexto: $result" }
    if ($result -notmatch 'IPv4|interface') { throw "Diagnóstico IPv4 sin contexto: $result" }
    Write-Host 'NOTSH Windows smoke: LAN, WLAN, IPv4, IPv6, WinHTTP, Winsock y Firewall regresaron a SST.'
} finally {
    if (-not $process.HasExited) { $process.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $dir -ErrorAction SilentlyContinue
}
