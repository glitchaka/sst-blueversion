# notsh — interfaz de red unificada de SST

`notsh` agrupa en una sola interfaz las capacidades que en sistemas Unix/Linux suelen estar repartidas entre `ip`, `ethtool`, `iw`, `nmcli`, `ss`, `resolvectl`, `nft`, `arp`/`ip neigh`, `ping` y `traceroute`.

No es un wrapper de `netsh.exe` y no debe depender de `netsh` para su funcionamiento básico.

## Separación con `net`

`net` sigue siendo la interfaz SST de alto nivel para descubrimiento, monitorización, presencia, inventario y proveedores de telemetría.

`notsh` se centra en el estado y configuración de la pila de red local del equipo.

```text
net
  scan / monitor / presence / identify / traffic / provider / usage

notsh
  link / addr / route / neigh / ss / wifi / dns / ping / trace / ...
```

No se eliminan los comandos existentes de `net`; esto mantiene compatibilidad con scripts y sesiones anteriores.

## Mapeo Unix → notsh

| Unix/Linux | notsh | Propósito |
| --- | --- | --- |
| `ip link` | `notsh link` | interfaces, estado administrativo y enlace |
| `ip addr` | `notsh addr` | direcciones locales por interfaz |
| `ethtool IFACE` | `notsh link IFACE` | velocidad, dúplex, carrier y capacidades |
| `ethtool -S IFACE` | `notsh stats IFACE` | contadores/errores del adaptador |
| `ip route` | `notsh route` | tabla de rutas |
| `ip neigh` | `notsh neigh` | vecinos IP/MAC |
| `ss` | `notsh ss` | sockets TCP/UDP y PID |
| `iw dev` | `notsh wifi` | adaptadores Wi-Fi y estado radio/enlace |
| `iw dev IFACE link` | `notsh wifi IFACE` | estado de asociación cuando Windows permita leerlo |
| `nmcli device status` | `notsh link` | resumen de dispositivos de red |
| `nmcli device wifi` | `notsh wifi scan` | redes visibles; sujeto a política de privacidad de Windows |
| `resolvectl` | `notsh dns` | resolución y configuración DNS |
| `ping` | `notsh ping` | ICMP echo |
| `traceroute` / `tracepath` | `notsh trace` | ruta hasta destino |
| `nft` | `notsh firewall` | estado/reglas de firewall |

## Implementado en la primera capa

```text
notsh link
notsh addr
notsh route
notsh neigh
notsh ss
notsh wifi
notsh dns HOST|IP
notsh ping HOST [-c N]
notsh trace HOST
notsh ports HOST [LISTA]
notsh conflicts [IP]
notsh diagnose
```

Aliases:

```text
interfaces              -> link
routes                  -> route
neighbors / arp         -> neigh
sockets / connections   -> ss
resolve                 -> dns
wlan / wireless         -> wifi
```

La primera capa reutiliza `NetworkService` y `NetworkProbe`; no duplica la lógica de `net`.

## Wi-Fi y privacidad de Windows

`notsh wifi` no debe exigir que el usuario active Ubicación para obtener el estado básico del adaptador.

El estado de interfaz se obtiene mediante la información local que SST ya consulta con IP Helper (`GetIfTable2`):

- adaptador;
- tipo Wi-Fi;
- estado administrativo;
- estado operativo;
- enlace;
- velocidad reportada;
- MAC;
- direcciones IP.

SSID/BSSID y escaneo de AP se consideran una capacidad separada porque Windows puede proteger esos datos detrás del permiso de ubicación.

Si esa información no está autorizada, `notsh` debe degradarse limpiamente:

```text
Wi-Fi: UP
IP: ...
Enlace: UP
SSID/BSSID: no disponible por política de privacidad de Windows
```

Nunca debe activar Ubicación ni modificar preferencias de privacidad automáticamente.

## Arquitectura objetivo

```text
notsh builtin
    |
    +-- NetworkService (funciones SST existentes)
    |
    +-- LocalNetworkService
           |
           +-- InterfaceProbe
           +-- RouteProbe
           +-- NeighborProbe
           +-- SocketProbe
           +-- WifiProbe
           +-- DnsProbe
           +-- FirewallProbe
           +-- LinkStatsProbe
```

Windows implementará esos puertos con APIs nativas siempre que sea razonable. Las herramientas de consola del sistema se usan solamente cuando no exista una API adecuada o cuando aporten una función que no compense reimplementar.

## Fases siguientes

### Fase 2 — equivalente a `ethtool`

Añadir:

```text
notsh link IFACE
notsh stats IFACE
```

Datos previstos:

- velocidad Tx/Rx;
- duplex cuando Windows lo exponga;
- MTU;
- carrier/link state;
- media type;
- errores y descartes;
- bytes/paquetes Tx/Rx;
- estado administrativo y operativo.

### Fase 3 — equivalente a `iw` / `nmcli`

Añadir un `WifiProbe` independiente:

```text
notsh wifi
notsh wifi IFACE
notsh wifi scan
notsh wifi profiles
```

El diseño debe distinguir datos libres de permisos de datos protegidos por la política de Ubicación de Windows.

### Fase 4 — equivalente a `resolvectl`

```text
notsh dns
notsh dns IFACE
notsh dns HOST
```

Mostrar:

- servidores DNS por interfaz;
- sufijos;
- estado DHCP/static;
- resolución directa/inversa;
- caché cuando sea viable.

### Fase 5 — equivalente a `nft`

```text
notsh firewall status
notsh firewall rules
notsh firewall ports
```

Debe reutilizar la infraestructura existente de `firewall` de Nwash, no crear una segunda implementación de Windows Firewall.

### Fase 6 — mutaciones

Las operaciones que cambien estado se añaden después de estabilizar las consultas read-only:

```text
notsh link IFACE up|down
notsh addr add|del ...
notsh route add|del ...
notsh dns set ...
notsh firewall ...
```

Cualquier mutación protegida debe pasar por el modelo de elevación de SST (`sudo`/broker); `notsh` no debe autoelevarse.
