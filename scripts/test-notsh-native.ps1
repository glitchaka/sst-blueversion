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
notsh show lan interfaces
echo SST_NOTSH_LAN_ALTERNATE_RETURNED
notsh wlan show interfaces
echo SST_NOTSH_WLAN_RETURNED
notsh show wlan interfaces
echo SST_NOTSH_WLAN_ALTERNATE_RETURNED
echo SST_CSV_LAN_BEGIN
notsh show lan interfaces --csv
echo SST_CSV_LAN_END
echo SST_CSV_WLAN_BEGIN
notsh wlan show interfaces --csv
echo SST_CSV_WLAN_END
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
notsh trace show sessions
echo SST_NOTSH_ETW_RETURNED
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
        'SST_NOTSH_LAN_ALTERNATE_RETURNED', 'SST_NOTSH_WLAN_ALTERNATE_RETURNED',
        'SST_NOTSH_IPV4_RETURNED', 'SST_NOTSH_IPV6_RETURNED',
        'SST_NOTSH_WINHTTP_RETURNED', 'SST_NOTSH_WINSOCK_RETURNED',
        'SST_NOTSH_FIREWALL_RETURNED',
        'SST_NOTSH_HTTP_IP_RETURNED', 'SST_NOTSH_HTTP_URL_RETURNED',
        'SST_NOTSH_ETW_RETURNED'
    )) {
        if (-not $result.Contains($marker)) {
            throw "No regresó de $marker. STDOUT: $result; STDERR: $errors"
        }
    }
    if ($result -match 'Uso: notsh show interfaces') {
        throw "NOTSH rechazó la sintaxis contextual LAN/WLAN: $result"
    }
    $lanMatch = [regex]::Match(
        $result, '(?s)SST_CSV_LAN_BEGIN\s*(.*?)\s*SST_CSV_LAN_END')
    if (-not $lanMatch.Success -or $lanMatch.Groups[1].Value -notmatch '^Nombre,Tipo,Estado,Administrativo,Enlace,Operativo,Velocidad_bps,MAC,IP,Adaptador,Diagnostico') {
        throw "NOTSH LAN --csv no produjo un encabezado CSV válido."
    }
    $lanRows = @($lanMatch.Groups[1].Value | ConvertFrom-Csv)
    # A machine with no physical Ethernet still emits valid header-only CSV.
    if ($lanRows.Count -gt 0 -and $null -eq $lanRows[0].Nombre) {
        throw "Las columnas de NOTSH LAN --csv no se pudieron analizar."
    }
    $wlanMatch = [regex]::Match(
        $result, '(?s)SST_CSV_WLAN_BEGIN\s*(.*?)\s*SST_CSV_WLAN_END')
    if (-not $wlanMatch.Success -or $wlanMatch.Groups[1].Value -notmatch '^Adaptador,Estado,Perfil,SSID,BSSID,Senal_pct,RX_Kbps,TX_Kbps,Autenticacion,8021X') {
        throw "NOTSH WLAN --csv no produjo CSV nativo."
    }
    if ($result -notmatch 'LAN|Dot3|Ethernet') { throw "Diagnóstico LAN sin contexto: $result" }
    if ($result -notmatch 'WLAN|Wi-Fi|WiFi') { throw "Diagnóstico WLAN sin contexto: $result" }
    if ($result -notmatch 'IPv4|interface') { throw "Diagnóstico IPv4 sin contexto: $result" }
    Write-Host 'NOTSH Windows smoke: LAN, WLAN, IPv4, IPv6, WinHTTP, Winsock y Firewall regresaron a SST.'
} finally {
    if (-not $process.HasExited) { $process.Kill() }
    Remove-Item -Recurse -Force -LiteralPath $dir -ErrorAction SilentlyContinue
}
