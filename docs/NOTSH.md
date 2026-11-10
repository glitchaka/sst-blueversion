# notsh — shell de red de SST

`notsh` es el comando principal de red de Shell Shock Tool. Sustituye conceptualmente a `netsh` dentro de SST y agrupa además capacidades que en Unix/Linux están repartidas entre `ip`, `ethtool`, `iw`, `nmcli`, `ss`, `resolvectl`, `nft`, `ip neigh`, `ping` y `traceroute`.

`net` se conserva únicamente como **alias de compatibilidad** del mismo builtin. No existe una segunda implementación.

```text
notsh ...
net ...      # alias del mismo comando
```

## Comportamiento al ejecutar `notsh` sin argumentos

`notsh` muestra un índice de funciones agrupadas por conectividad, diagnóstico e inventario. No ejecuta automáticamente una consulta de interfaces ni termina la ayuda al llegar a Wi-Fi.

Para consultar el equipo se utilizan subcomandos explícitos:

```text
notsh                 # índice de comandos de red
notsh interfaces      # todos los adaptadores
notsh wlan            # ayuda del contexto Wi-Fi
notsh lan             # ayuda del contexto Ethernet
notsh help            # índice de comandos de red
```

## Contextos nativos de Windows

Notsh no ejecuta procesos Netsh, CMD ni PowerShell. Los contextos actualmente implementados usan consultas directas al sistema operativo:

| Consulta | API | Interpretación |
| --- | --- | --- |
| `notsh interfaces` | IP Helper | Inventario completo, incluidos adaptadores virtuales |
| `notsh lan show interfaces` | IP Helper + NDIS + Service Control Manager + Dot3 API | Ethernet físico, enlace, IP, dot3svc, interfaces Dot3 y configuración 802.1X; no demuestra autorización NAC |
| `notsh wlan show interfaces` | WLAN API | Estado Wi-Fi, conexión, SSID, BSSID, señal, configuración 802.1X |
| `notsh wlan show profiles` | WLAN API | Nombres de perfiles guardados sin exponer claves |
| `notsh wlan show networks` | WLAN API | Redes visibles si Windows permite la consulta |
| `notsh interface ipv4 show interfaces` | IP Helper | Tabla IPv4 por interfaz |
| `notsh interface ipv6 show interfaces` | IP Helper | Tabla IPv6 por interfaz |

La vista LAN incluye el estado real del servicio Wired AutoConfig (`dot3svc`) consultado mediante el gestor de servicios de Windows. Este estado NO demuestra una autenticación exitosa.

**Límites explícitos:** Las interfaces y perfiles cableados se consultan ahora mediante Dot3 API sin exponer el XML del perfil ni secretos. El estado real de autorización de 802.1X/NAC no se infiere de un perfil configurado. `notsh lan show settings` todavía requiere un backend especializado. Los contextos `advfirewall`, `winhttp`, `winsock`, `http` y controles de `trace` quedan pendientes de implementación nativa. No se presentan adaptadores genéricos como datos de autenticación, ni se usa un envoltorio de `netsh.exe`. Las consultas Wi-Fi respetan permisos de Ubicación y las políticas de Windows.

## Funciones existentes de SST

Toda la funcionalidad que antes se invocaba mediante `net` sigue disponible porque `net` y `notsh` son el mismo builtin:

```text
notsh interfaces
notsh connections
notsh routes
notsh dns HOST|IP
notsh ping HOST [-c N]
notsh trace HOST
notsh neighbors
notsh conflicts [IP]
notsh ports HOST [LISTA]
notsh scan [RED]
notsh monitor [RED]
notsh presence
notsh identify OBJETIVO
notsh traffic
notsh usage
notsh provider ...
```

Y siguen funcionando los scripts existentes:

```text
net scan
net monitor
net interfaces
```

## Alias tipo Unix

```text
notsh link       -> interfaces
notsh addr       -> interfaces
notsh route      -> routes
notsh neigh      -> neighbors
notsh ss         -> connections
notsh resolve    -> dns
notsh wifi       -> wlan
```

## Mapeo Unix → notsh

| Unix/Linux | notsh | Propósito |
| --- | --- | --- |
| `ip link` | `notsh link` | interfaces y enlace |
| `ip addr` | `notsh addr` | direcciones locales |
| `ip route` | `notsh route` | tabla de rutas |
| `ip neigh` | `notsh neigh` | vecinos IP/MAC |
| `ss` | `notsh ss` | sockets TCP/UDP y PID |
| `iw dev` | `notsh wlan show interfaces` | adaptadores Wi-Fi sin datos protegidos |
| `resolvectl` | `notsh dns` | DNS |
| `ping` | `notsh ping` | ICMP echo |
| `traceroute` | `notsh trace` | ruta hasta destino |

## Diseño

```text
notsh / net(alias)
        |
        +-- NetworkService
        |     +-- diagnóstico
        |     +-- discovery/scan
        |     +-- monitor/presence
        |     +-- traffic
        |     +-- providers
        |
        +-- NetworkProbe
              +-- WindowsNetworkProbe
                    +-- IP Helper APIs
```

La vista nativa `notsh interfaces` y los subcomandos de red implementados se comunican directamente con Windows; no invocan el ejecutable de Netsh.

## Siguientes capacidades

El mismo comando absorberá progresivamente funciones equivalentes a:

```text
ethtool   -> notsh stats / notsh link IFACE
iw/nmcli  -> notsh wlan ...
resolvectl-> notsh dns ...
nft       -> notsh firewall ...
```

Las operaciones que cambien configuración deberán usar el modelo de elevación de SST (`sudo`/broker) cuando Windows lo exija. `notsh` no debe autoelevarse ni modificar permisos de privacidad.
