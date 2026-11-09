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

## Compatibilidad tipo netsh

```text
notsh wlan show interfaces
notsh lan show interfaces
notsh show interfaces
```

Como `net` es alias, también funcionan:

```text
net wlan show interfaces
net lan show interfaces
net show interfaces
```

### Interfaces LAN físicas

La consulta `notsh lan show interfaces` selecciona únicamente adaptadores Ethernet físicos mediante los metadatos de Windows IP Helper/NDIS. No incluye Wi-Fi, miniports WAN, filtros WFP ni conexiones virtuales aunque el sistema las clasifique genéricamente como Ethernet. El resumen distingue adaptadores deshabilitados, enlace desconectado, enlace con IP y enlace sin IP. Una señal física activa no confirma por sí sola autenticación NAC, direccionamiento válido ni acceso a recursos.

`notsh show interfaces` mantiene la vista completa, incluida la conectividad virtual.

### Wi-Fi sin permiso de ubicación

`notsh wlan show interfaces` no ejecuta `netsh.exe` ni usa `WlanQueryInterface` para consultar SSID/BSSID.

El estado básico del adaptador se obtiene mediante el `WindowsNetworkProbe` de SST usando IP Helper (`GetIfTable2`) y las direcciones locales ya disponibles para SST. Por tanto puede mostrar, sin pedir Ubicación:

- adaptador Wi-Fi;
- estado administrativo;
- estado operativo;
- enlace;
- velocidad reportada;
- MAC;
- direcciones IP.

SSID, BSSID y escaneo de puntos de acceso son datos separados. Windows puede protegerlos detrás del permiso de Ubicación, por lo que `notsh wlan show interfaces` **no los consulta** y nunca activa ni solicita ese permiso automáticamente.

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

No se llama a `netsh.exe` para obtener interfaces.

## Siguientes capacidades

El mismo comando absorberá progresivamente funciones equivalentes a:

```text
ethtool   -> notsh stats / notsh link IFACE
iw/nmcli  -> notsh wlan ...
resolvectl-> notsh dns ...
nft       -> notsh firewall ...
```

Las operaciones que cambien configuración deberán usar el modelo de elevación de SST (`sudo`/broker) cuando Windows lo exija. `notsh` no debe autoelevarse ni modificar permisos de privacidad.
