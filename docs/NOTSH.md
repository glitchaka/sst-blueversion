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
notsh wlan            # adaptadores Wi-Fi
notsh lan             # adaptadores Ethernet
notsh help            # índice de comandos de red
```

## Contextos de Windows: compatibilidad con Netsh

NOTSH conserva las operaciones propias de SST y las rutas de comandos de Windows. No son la misma cosa.

Los contextos `lan`, `wlan`, `interface`, `advfirewall`, `winhttp`, `winsock`, `http` y las funciones de seguimiento de `trace` utilizan el **backend de compatibilidad oficial `netsh.exe`** ubicado en System32. Se invoca directamente, **sin CMD ni PowerShell**, con los permisos y las restricciones que tenga el proceso SST. Esto garantiza que el resultado y las reglas de cada subcomando corresponden a Windows; no se sustituyen por un listado genérico de interfaces.

```text
notsh lan show interfaces                → netsh lan show interfaces
notsh lan show profiles                  → netsh lan show profiles
notsh lan show settings                  → netsh lan show settings
notsh wlan show interfaces               → netsh wlan show interfaces
notsh wlan show profiles                 → netsh wlan show profiles
notsh wlan show networks                 → netsh wlan show networks
notsh interface ipv4 show interfaces     → netsh interface ipv4 show interfaces
notsh interface ipv6 show interfaces     → netsh interface ipv6 show interfaces
```

`notsh lan show interfaces` presenta el estado del sistema de autenticación cableada de Windows, incluida 802.1X cuando corresponde. No deduce una autorización NAC a partir de la presencia de cable o dirección IP. Si Wired AutoConfig está detenido, restringido o no entrega el estado, se muestra el resultado correspondiente de Windows.

`notsh wlan show interfaces` presenta el estado Wi-Fi, SSID/BSSID, perfil y modo de autenticación cuando Windows permite consultarlos. Se respetan los permisos de privacidad, como Ubicación.

Los contextos sin argumentos muestran ayuda, sin abrir una sesión interactiva adicional. Las operaciones de modificación de configuración mantienen los requisitos de elevación de Windows. Las políticas de bloqueo de ejecutables también pueden restringir este backend.

`notsh interfaces` es **una operación distinta**, implementada con IP Helper: muestra el inventario completo de adaptadores y conexiones lógicas. No equivale a `netsh lan show interfaces`.
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

La vista nativa `notsh interfaces` no ejecuta procesos externos; los contextos compatibles con Netsh sí utilizan directamente el ejecutable de Windows.

## Siguientes capacidades

El mismo comando absorberá progresivamente funciones equivalentes a:

```text
ethtool   -> notsh stats / notsh link IFACE
iw/nmcli  -> notsh wlan ...
resolvectl-> notsh dns ...
nft       -> notsh firewall ...
```

Las operaciones que cambien configuración deberán usar el modelo de elevación de SST (`sudo`/broker) cuando Windows lo exija. `notsh` no debe autoelevarse ni modificar permisos de privacidad.
