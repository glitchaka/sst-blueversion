# Shell Shock Tool (SST)

**Shell Shock Tool**, o **SST**, es una consola portable para Windows escrita en Rust. Combina una terminal Win32 propia, **Nwash** —su intérprete basado en la sintaxis y semántica portable de Bash 5.3 y adaptado deliberadamente a Windows—, utilidades Unix integradas y herramientas de soporte técnico, diagnóstico, inventario y red.


[📘 Manual de comandos SST/Nwash](MANUAL.md)

El ejecutable actual se llama:

```text
sst.exe
```

SST está pensado para poder llevarse como herramienta portable y trabajar desde una única consola sin depender de PowerShell para las tareas que ya incorpora de forma nativa.

---

## Construcción

```powershell
cargo build --release
```

El ejecutable se genera en:

```text
target\release\sst.exe
```

Durante la compilación, `build.rs` incorpora la Nerd Font, el paquete oficial de Helix, el diccionario ortográfico es-CL y los registros IEEE MA-L/MA-M/MA-S utilizados para identificación de fabricantes MAC.

Para proporcionar copias locales de los registros IEEE pueden definirse:

```text
SST_IEEE_MAL_CSV
SST_IEEE_MAM_CSV
SST_IEEE_MAS_CSV
```

También existen `SST_NERD_FONT_FILE` y `SST_HELIX_ARCHIVE` para sustituir esos recursos durante la construcción.

No es necesario distribuir los directorios internos de Cargo como `build/`, `deps/`, `incremental/` o los archivos `.pdb`/`.d` para ejecutar SST.

---

# Mapa de herramientas

SST tiene cuatro grupos principales de herramientas:

1. **Shell Bash-compatible**: scripting, variables, pipes, redirecciones, jobs, arrays, funciones, traps, historial y completion.
2. **Utilidades Unix integradas**: archivos, texto, compresión, hashes, búsqueda y navegación.
3. **Herramientas SST para Windows y red**: sistema, red, inventario, Wake-on-LAN, dominio, switches, tráfico y diagnósticos.
4. **Herramientas auxiliares**: configuración portable, traducción de rutas y editor Helix integrado.

Puedes obtener ayuda desde la propia shell con:

```bash
help
help COMANDO
```

## Cambios recientes de `main`

Esta documentación refleja el código actual, incluidos estos cambios:

- **Descubrimiento de red por evidencia múltiple:** `net scan` y `net monitor` usan ARP activo, ICMP y TCP. Una respuesta ARP basta para detectar teléfonos/IoT aunque bloqueen ping o no tengan servicios TCP.
- **Identificación MAC local:** cada host se clasifica como `global`, `local/private`, `multicast`, `broadcast` o `unknown`. Las MAC globales se resuelven contra los registros IEEE **MA-L, MA-M y MA-S**; las MAC privadas/aleatorias no reciben un fabricante inventado.
- **Monitor con histéresis:** un host debe faltar en tres ciclos consecutivos antes de generar un evento de desconexión, reduciendo falsos `+/-` por una respuesta perdida.
- **Mensajes SST interactivos entre monitores:** con `net monitor` abierto, `Enter` abre `Mensaje>`; se escribe el texto y otro `Enter` lo publica por UDP broadcast. Otros SST con `net monitor` abierto muestran la IP emisora y el texto. Sirve como easter egg y como comprobación práctica de comunicación LAN/broadcast/firewall.
- **Tiempo de respuesta corregido:** la columna de respuesta mide el probe que confirmó presencia y ya no incluye el tiempo de reverse DNS.
- **`net identify`:** identifica por IP, MAC o nombre inventariado y reúne hostname, MAC, scope, fabricante IEEE, inventario, método de descubrimiento y último avistamiento.
- **Registro directo por IP:** `device add IP NOMBRE` resuelve la MAC por ARP y la incorpora al inventario.
- **`net traffic --unsigned`:** añade estado Authenticode por proceso y permite filtrar ejecutables cuya firma no sea válida.
- **`net usage` operativo para HTTP JSON normalizado:** los proveedores `generic` y `openwrt` pueden entregar contadores por cliente; existen filtros, JSON/CSV y modo watch.
- **Credenciales de `intel` desde `config/sstrc`:** las claves pueden declararse con `export`; SST consulta primero el entorno de Windows y después `sstrc`.
- **`intel update` y caché TTL:** limpia entradas expiradas y muestra el estado/capacidad de las fuentes habilitadas.
- **Terminal Slint:** restaurada y ampliada la selección/copia; arrastre, doble clic por palabra, triple clic por línea, `Ctrl+C` contextual, `Ctrl+Shift+C/V`, `Ctrl+Insert`, `Shift+Insert` y clic derecho contextual.
- **Helix-SST 0.2.1:** tema Gruvbox Dark, corrector ortográfico offline es-CL para texto/Markdown, sugerencias con `F2` y `Alt+d` para insertar `—`.
- **Nwash Windows:** están registrados y exportados `eventlog`, `service`, `registry`, `process`, `acl`, `pnp`, `task`, `session`, `share`, `firewall` y `power`.
- **Integridad de terminal:** restaurados `AlternateScreenGuard` y `output_sender()`, requeridos por consumidores reales del editor/TUI.
- **Correcciones de integración:** corregidos el escape Authenticode y el mensaje de formato del backend de `net usage`.

## Referencia rápida de comandos SST/Nwash

| Comando | Función | Ejemplo |
|---|---|---|
| `sys` | Sistema, procesos, servicios, usuarios, impresoras, drivers, eventos, registro, tareas y triage | `sys info` |
| `net` | Interfaces, conexiones, DNS, descubrimiento, identificación, presencia, tráfico y proveedores | `net identify 10.11.24.20` |
| `device` | Inventario local por MAC o IP | `device add 10.11.24.20 MI-CELULAR` |
| `wol` | Wake-on-LAN | `wol NOTEBOOK-01` |
| `domain` | Estado de dominio | `domain status PC-01 --verify` |
| `switch` | MAC → switch → puerto por SNMP | `switch locate NOTEBOOK-01` |
| `diag` | Diagnósticos compuestos | `diag network` |
| `triage` | Resumen de señales locales | `triage` |
| `intel` | Fuentes/caché/lookup de inteligencia | `intel lookup INDICADOR` |
| `sudo` / `runas` | Elevación Administrador → SYSTEM → TrustedInstaller | `sudo --status` |
| `eventlog` | Windows Event Log | `eventlog read System --count 20` |
| `service` | Control de servicios | `service restart Spooler` |
| `registry` | Lectura/escritura/import/export de Registro | `registry get "HKLM\SOFTWARE"` |
| `process` | Listado, detalle, árbol y terminación | `process tree 1234` |
| `acl` | ACL NTFS | `acl show C:\Datos` |
| `pnp` | Dispositivos Plug and Play | `pnp list --problem` |
| `task` | Tareas programadas | `task show "\MiTarea"` |
| `session` | Sesiones locales/RDP | `session users` |
| `share` | Recursos SMB | `share list` |
| `firewall` | Perfiles/reglas de Windows Firewall | `firewall status` |
| `power` | Apagado, reinicio, logoff, hibernación | `sudo power restart` |
| `helix` / `hx` / `helix-sst` | Editor Helix-SST | `helix notas.txt` |
| `tour` | Menú TUI de ejemplos ejecutables de Nwash/SST | `tour` |
| `config` | Configuración portable de shell | `config edit` |
| `sst-path` | Traducción de rutas Unix → Windows | `sst-path /c/Windows/System32` |

---

# Herramientas SST

## `sys` — sistema y administración local

`sys` agrupa consultas del equipo y herramientas de auditoría local.

### Información del equipo

| Comando | Qué hace | Ejemplo |
|---|---|---|
| `sys info` | Resumen del sistema operativo, CPU, memoria y equipo. | `sys info` |
| `sys processes` / `sys ps` | Procesos y consumo. | `sys processes` |
| `sys top` | TUI de procesos; `c` ordena por CPU, `m` por memoria, `q` sale. | `sys top` |
| `sys disks` / `sys df` | Discos, capacidad y uso. | `sys disks` |
| `sys memory` / `sys free` | Memoria física y swap. | `sys memory` |
| `sys uptime` | Tiempo desde el último arranque. | `sys uptime` |
| `sys hostname` | Nombre del equipo. | `sys hostname` |
| `sys whoami` | Usuario actual. | `sys whoami` |
| `sys uname [-a]` | Identificación del sistema. | `sys uname -a` |
| `sys kill PID [--tree]` | Termina un PID o su árbol. | `sys kill 8124 --tree` |
| `sys fetch [--small|--full]` | Resumen visual con el logo SST coloreado; `fetch`/`neofetch`/`fastfetch` son equivalentes. | `sys fetch --full` |
| `sys inspect PID [--deep]` | Inspección local de proceso. | `sys inspect 8124 --deep` |
| `sys why PID` | Explica la clasificación/señales observadas. | `sys why 8124` |
| `sys diff PID` | Compara identidad/parent/command line con historial. | `sys diff 8124` |
| `sys suspicious` | Lista procesos que superan reglas actuales de atención. | `sys suspicious` |
| `sys safe PID [PID ...]` | Confía la relación exacta ejecutable ← padre actual; no desactiva otras señales. | `sys safe 4912 6192` |
| `sys safe list` | Lista relaciones de linaje confiables. | `sys safe list` |
| `sys safe remove ID [...]` | Elimina una relación de confianza por ID. | `sys safe remove 3` |
| `sys startup` | Revisa puntos de inicio observables. | `sys startup` |
| `sys persistence` | Vista de persistencia basada en startup + tareas/servicios existentes. | `sys persistence` |
| `sys services --impact` | Relaciona servicios con PID, CPU y RAM. | `sys services --impact` |
| `sys inspect PID --broker` | Inspección mediante broker autenticado. | `sys inspect 8124 --broker` |
| `sys suspend PID --start-time FILETIME` | Suspensión mediante broker. | `sudo sys suspend 8124 --start-time 133...` |
| `sys resume PID --start-time FILETIME` | Reanuda mediante broker. | `sudo sys resume 8124 --start-time 133...` |
| `sys kill PID --broker --start-time FILETIME` | Terminación broker con protección contra reutilización de PID. | `sudo sys kill 8124 --broker --start-time 133...` |

También existen como comandos directos:

```text
ps
top
df
free
uptime
hostname
whoami
uname
kill
fetch
neofetch
fastfetch
```

Al abrir la terminal gráfica de SST, `fetch` se ejecuta automáticamente antes del preload de seguridad y del primer prompt. El logo mostrado está construido como arte ANSI coloreado a partir de `assets/sst-neofetch.png` y conserva la paleta del branding SST.

La ventana solicita foco de teclado al crearse y, en Windows, refuerza la activación/foco nativo tras `show()`; no debería requerir un clic previo para comenzar a escribir. El ejecutable usa `assets/sst-icon.ico`, generado desde el icono oficial `assets/sst-icon.png`.

### Servicios de Windows

```bash
sys services
sys services --running
sys services --stopped
sys services NOMBRE
```

Permite listar servicios, filtrar por estado o consultar un servicio concreto. Usa `sc.exe` como backend y actualmente es de consulta.

### Usuarios

```bash
sys users
sys users USUARIO
sys users --domain
sys users USUARIO --domain
```

Consulta cuentas locales o del dominio mediante las herramientas nativas de Windows.

### Drivers y dispositivos PnP

```bash
sys drivers
sys drivers --verbose
sys drivers --signed
sys drivers --csv
sys drivers --pnp
sys drivers --devices
```

- `--verbose`: información detallada de los drivers.
- `--signed`: añade información de firma.
- `--csv`: salida CSV.
- `--pnp`: enumera paquetes de drivers PnP.
- `--devices`: enumera dispositivos PnP conectados.

### Impresoras

```bash
sys printers
sys printers --default
sys printer
sys printer "NOMBRE"
sys printer "NOMBRE" --ip
```

Consulta las impresoras instaladas mediante la API nativa del spooler de Windows.
Muestra la impresora predeterminada, servidor de impresión, recurso compartido,
puerto, driver, ubicación e IP cuando el puerto TCP/IP contiene una dirección o
un hostname resoluble. No depende de PowerShell.

### Windows Event Log

```bash
sys events
sys events Application
sys events --log Security
sys events --count 50
sys events --query XPATH
sys events --format text
sys events --format xml
sys events --logs
sys events --publishers
```

Permite consultar eventos recientes, elegir log, limitar cantidad, aplicar filtros XPath, obtener XML y enumerar logs o publishers.

### Registro de Windows

```bash
sys registry "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
sys registry CLAVE --value NOMBRE
sys registry CLAVE --default
sys registry CLAVE --recursive
sys registry CLAVE --find TEXTO
sys registry CLAVE --find TEXTO --keys
sys registry CLAVE --find TEXTO --data
```

La integración actual es **de solo lectura** y utiliza `reg.exe query`.

### Tareas programadas

```bash
sys tasks
sys tasks --verbose
sys tasks --csv
sys tasks NOMBRE
sys tasks NOMBRE --xml
```

Consulta tareas programadas, detalles o su definición XML mediante `schtasks.exe`.

---

## Nwash — primitivas Windows

Nwash expone operaciones de Windows como builtins componibles. Todos aceptan `-h`, `--help` o `help` cuando el handler lo contempla.

### `eventlog`

Subcomandos:

```bash
eventlog list
eventlog publishers
eventlog info LOG
eventlog read [LOG] [--count N] [--query XPATH] [--xml]
eventlog export LOG ARCHIVO [--query XPATH] [--overwrite]
eventlog clear LOG [--backup ARCHIVO]
```

Ejemplos:

```bash
eventlog read System --count 50
eventlog read Security --query "*[System[(Level=2)]]"
eventlog export Application app.evtx --overwrite
sudo eventlog clear Application --backup Application-backup.evtx
```

### `service`

```bash
service list [--running|--stopped]
service status NOMBRE
service start NOMBRE
service stop NOMBRE
service pause NOMBRE
service resume NOMBRE
service restart NOMBRE
```

Ejemplos:

```bash
service list --running
service status Spooler
sudo service restart Spooler
```

### `registry`

```bash
registry get CLAVE [--value NOMBRE|--default] [--recursive]
registry set CLAVE NOMBRE DATO [--type TIPO]
registry set-default CLAVE DATO [--type TIPO]
registry delete CLAVE [--value NOMBRE|--default|--key]
registry export CLAVE ARCHIVO [--overwrite]
registry import ARCHIVO
```

`TIPO`: `REG_SZ`, `REG_EXPAND_SZ`, `REG_DWORD`, `REG_QWORD`, `REG_MULTI_SZ`, `REG_BINARY`.

Ejemplos:

```bash
registry get "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion" --value ProductName
sudo registry set "HKCU\Software\SST" Enabled 1 --type REG_DWORD
registry export "HKCU\Software\SST" sst.reg --overwrite
sudo registry import sst.reg
```

### `process`

```bash
process list
process info PID
process tree PID
process kill PID [--tree] [--force]
```

Ejemplos:

```bash
process info 8124
process tree 8124
sudo process kill 8124 --tree --force
```

`process tree` usa el árbol de procesos obtenido desde Rust/`sysinfo`; no depende de WMIC.

### `acl`

```bash
acl show RUTA
acl grant RUTA USUARIO PERMISO
acl deny RUTA USUARIO PERMISO
acl revoke RUTA USUARIO
acl inherit RUTA on|off
acl reset RUTA
```

Permisos habituales: `F`, `M`, `RX`, `R`, `W`.

Ejemplos:

```bash
acl show "C:\Datos"
sudo acl grant "C:\Datos" "DOMINIO\usuario" RX
sudo acl inherit "C:\Datos" off
```

### `pnp`

```bash
pnp list [--connected|--disconnected|--problem]
pnp info INSTANCE_ID
pnp enable INSTANCE_ID
pnp disable INSTANCE_ID
pnp restart INSTANCE_ID
pnp scan
```

Ejemplos:

```bash
pnp list --problem
pnp info "PCI\VEN_..."
sudo pnp scan
```

### `task`

```bash
task list [--verbose|--csv]
task show NOMBRE [--xml]
task run NOMBRE
task end NOMBRE
task enable NOMBRE
task disable NOMBRE
task delete NOMBRE
```

Ejemplos:

```bash
task list --verbose
task show "\Microsoft\Windows\Defrag\ScheduledDefrag" --xml
sudo task disable "\MiTarea"
```

### `session`

```bash
session list
session users
session logoff ID
session message ID TEXTO
```

Ejemplos:

```bash
session users
sudo session message 2 "Mantenimiento en 10 minutos"
sudo session logoff 2
```

### `share`

```bash
share list
share sessions
share files
share add NOMBRE RUTA
share remove NOMBRE
```

Ejemplos:

```bash
share list
sudo share add Datos "C:\Datos"
sudo share remove Datos
```

### `firewall`

```bash
firewall status
firewall rules
firewall rule NOMBRE
firewall enable PROFILE
firewall disable PROFILE
```

`PROFILE`: `domain`, `private`, `public`, `all`.

Ejemplos:

```bash
firewall status
firewall rule "Remote Desktop - User Mode (TCP-In)"
sudo firewall enable domain
```

### `power`

```bash
power shutdown [--force]
power restart [--force]
power logoff [--force]
power hibernate
```

Ejemplos:

```bash
sudo power restart
power logoff
power hibernate
```

El intérprete expone `$NWASH_VERSION`, `$NWASH_BASH_BASE` y `$NWASH_PLATFORM`. Nwash no intenta reproducir infraestructura binaria interna de GNU Bash sin valor portable en Windows, como `enable -f`/`enable -d`.

## Elevación y privilegios

`sudo` y su alias `runas` usan la elevación nativa de SST; `runas` **no invoca `runas.exe`**.

```bash
sudo
sudo --status
sudo COMANDO [argumentos]
sudo --system COMANDO [argumentos]
sudo --trustedinstaller COMANDO [argumentos]
runas COMANDO [argumentos]
```

La escalera de sesión es:

```text
USER / ADMIN_FILTERED
        ↓ sudo
ADMINISTRATOR
        ↓ sudo
LOCAL_SYSTEM
        ↓ sudo
TRUSTEDINSTALLER
```

Ejemplos:

```bash
sudo --status
sudo service restart Spooler
sudo --system whoami
sudo --trustedinstaller registry get "HKLM\SOFTWARE"
```

`sudo --status` muestra nivel, identidad, integridad y privilegios `Se*` del token. Cuando se ejecuta `sudo` sin comando, SST intenta elevar la **sesión** al siguiente nivel. Las variantes `--system` y `--trustedinstaller` ejecutan un comando concreto con ese token.

## Triage de seguridad local

SST incorpora un triage local explicable. El motor separa rendimiento de señales
de seguridad y conserva historial en `data/security.db`.

Comandos principales:

```bash
triage
sys why PID
sys inspect PID
sys inspect PID --deep
sys diff PID
sys suspicious
sys startup
sys persistence
sys services --impact
intel status
intel sources
intel update
intel lookup INDICADOR
```

La clasificación operacional es:

```text
NORMAL
PERFORMANCE
ATTENTION
SUSPICIOUS
ALERT
```

### Confianza local de linaje

Cuando SST marca un proceso únicamente porque su padre es nuevo o históricamente raro, el operador puede registrar esa **pareja exacta** como conocida:

```bash
sys safe PID
sys safe PID1 PID2 PID3
sys safe list
sys safe remove ID
```

Ejemplo:

```bash
sys safe 4912 6192 13792
```

SST guarda en `data/security.db` la relación completa `child_exe ← parent_exe`. Esto neutraliza solamente la anomalía histórica de ese padre. **No declara el proceso globalmente seguro** y no suprime señales independientes como ejecución sospechosa, Authenticode, SHA-256, red, persistencia o inteligencia externa.


### `intel` — fuentes, caché y credenciales

Subcomandos:

| Subcomando | Función | Ejemplo |
|---|---|---|
| `intel status` | Estado, adapter, TTL y disponibilidad de autenticación. | `intel status` |
| `intel sources` | Muestra el registro de fuentes configurado en `data/security.sources`. | `intel sources` |
| `intel update` | Elimina caché expirada y muestra capacidad/estado de fuentes habilitadas. | `intel update` |
| `intel lookup INDICADOR` | Consulta listas locales, caché y adapters externos habilitados. | `intel lookup 44d88612fea8a8f36de82e1278abb02f` |

Las credenciales pueden declararse en `config/sstrc`:

```bash
export SST_MALWAREBAZAAR_AUTH_KEY='...'
export SST_THREATFOX_AUTH_KEY='...'
export SST_URLHAUS_AUTH_KEY='...'
```

El registro `data/security.sources` **no contiene las claves**: guarda nombres como `auth_env=SST_THREATFOX_AUTH_KEY`. Para esas credenciales, `intel` consulta primero el entorno real de Windows y, si no existe allí, lee el mismo nombre desde `config/sstrc`.



Los collectors todavía no conectados se muestran como información incompleta;
no se interpretan como evidencia negativa ni como señal de seguridad.

### Broker LocalSystem

Las operaciones privilegiadas de proceso usan el servicio cerrado
`SSTPrivilegedBroker`. El protocolo v2 sólo admite:

```text
INSPECT
SUSPEND
RESUME
KILL
```

No existe una operación genérica para ejecutar comandos como SYSTEM.

```bash
sys inspect 8124 --broker
sudo sys suspend 8124 --start-time FILETIME
sudo sys resume 8124 --start-time FILETIME
sudo sys kill 8124 --broker --start-time FILETIME
```

`INSPECT` puede comenzar sólo con PID y devuelve el FILETIME exacto del proceso.
Las mutaciones exigen `PID + FILETIME` para rechazar reutilización de PID.

La instalación del broker es explícita y se documenta en
`docs/security-broker.md`; SST portable no instala ni inicia el servicio por sí
solo.

---

## `net` — diagnóstico, descubrimiento y tráfico de red

### Interfaces

```bash
net interfaces
```

Muestra las interfaces de red del equipo.

### Conexiones

```bash
net connections
```

Muestra protocolo, dirección local, dirección remota, estado y PID de las conexiones activas.

### Rutas

```bash
net routes
```

Muestra la tabla de rutas de Windows.

### DNS

```bash
net dns HOST
net dns IP
```

Resuelve nombres a direcciones IP y realiza resolución inversa cuando se entrega una IP.

### Ping

```bash
net ping HOST
net ping HOST -c 10
```

Envía ICMP desde el motor nativo de SST. `-c` acepta entre 1 y 100 intentos.

### Trace

```bash
net trace HOST
net traceroute HOST
```

Realiza un trazado ICMP de hasta 30 saltos.

### Vecinos / ARP

```bash
net neighbors
net arp
```

Muestra la tabla de vecinos conocida por el equipo enriquecida con **MAC, tipo/scope y fabricante IEEE** cuando la dirección es global. Las MAC `local/private` se muestran como tales y no reciben un fabricante atribuido.

### Escaneo de puertos

```bash
net ports HOST
net ports HOST 22,80,443,445,3389
```

Comprueba conectividad TCP contra los puertos indicados. Si no se especifican puertos usa un conjunto habitual.

### Descubrimiento de red

```bash
net scan
net scan 192.168.1.0/24
net scan --unknown
net scan --authorized
net scan --known
net scan --names
net scan --json
net scan --csv
```

Escanea una red IPv4 y relaciona los equipos encontrados con:

- dirección IP;
- hostname;
- MAC;
- tipo/scope de MAC;
- fabricante cuando existe una asignación IEEE válida;
- método que confirmó presencia (`arp`, `icmp` o `tcp:PUERTO`);
- tiempo de respuesta del probe;
- estado conocido/desconocido;
- nombre del inventario SST, si existe.

El descubrimiento usa **ARP activo primero** en IPv4 local mediante `SendARP`. Si obtiene una MAC, el host se considera presente aunque no tenga puertos TCP escuchando y aunque bloquee ICMP. Si ARP no responde, SST intenta ICMP y después conectividad TCP sobre puertos habituales.

La identificación MAC distingue:

- `global`: puede resolverse contra IEEE MA-L/MA-M/MA-S;
- `local/private`: dirección administrada localmente, común en MAC privadas/aleatorias de teléfonos; SST no atribuye fabricante por OUI;
- `multicast`;
- `broadcast`;
- `unknown`: no se obtuvo MAC.

La columna `RESP` ya no incluye reverse DNS: representa únicamente el tiempo del probe que confirmó presencia.

La salida normal prioriza las columnas **IP** y **NOMBRE**. SST intenta resolver el hostname del equipo y, si no hay resolución disponible pero el dispositivo está inventariado, utiliza el nombre guardado en el inventario.

Sin red explícita, SST intenta determinar la red IPv4 local y usa una /24. Por seguridad, el escaneo está limitado a redes /20 o más pequeñas.

`--unknown` muestra solo equipos que no están en el inventario.  
`--authorized` y `--known` muestran solo equipos conocidos.  
`--names` muestra únicamente IP y nombre del equipo, como un IP Scanner simple:

```text
IP               NOMBRE
192.168.1.10     PC-RECEPCION
192.168.1.21     NOTEBOOK-01
```

### Monitor de presencia

```bash
net monitor
net monitor 192.168.1.0/24
net monitor --unknown
net monitor -m "Prueba desde soporte"
net monitor 10.11.24.0/24 --message "Estoy en biblioteca"
net monitor 10.11.24.0/24 --say "Hola SST"
```

Abre una vista TUI que repite el mismo descubrimiento ARP/ICMP/TCP de `net scan` y registra:

- aparición de equipos;
- desaparición confirmada;
- cambios de IP;
- primera y última vez vistos;
- fabricante/tipo de MAC y método de descubrimiento.

Para evitar flapping, un equipo no se declara desconectado por un único ciclo fallido: SST exige **tres fallos consecutivos** antes de emitir el evento `-`. Durante esa ventana conserva la última identidad estable en pantalla y muestra `miss 1/3` o `miss 2/3` en vez de fingir que la detección fue positiva.

#### Mensajes SST entre monitores

La forma normal de conversar es **sin salir del monitor**:

```text
[Enter] mensaje · [q/Esc] salir

... monitor ...

Mensaje> Prueba desde soporte_
[Enter] enviar · [Esc] cancelar
```

Comportamiento:

- fuera del prompt, `Enter` abre `Mensaje>`;
- se escribe el texto normalmente;
- otro `Enter` lo envía y vuelve a la vista del monitor;
- dentro del prompt, `Esc` cancela la edición;
- fuera del prompt, `q` o `Esc` salen de `net monitor`;
- mientras se escribe, la letra `q` se trata como texto y no cierra el monitor;
- el último mensaje enviado queda como mensaje publicado y SST lo vuelve a anunciar periódicamente mientras el monitor permanezca abierto.

También se mantienen `-m`, `--message` y `--say` como atajos opcionales para **arrancar** el monitor con un mensaje inicial:

```bash
net monitor 10.11.24.0/24 -m "Prueba desde soporte"
```

Los mensajes admiten hasta 120 caracteres y usan **UDP broadcast al puerto 43837** del segmento monitorizado.

Cualquier otro SST que esté ejecutando `net monitor` en el mismo segmento puede mostrar:

```text
Mensajes SST:
10.11.24.13     anscharve.uautonoma.cl      1s   Prueba desde soporte
```

La IP mostrada se toma de la **dirección de origen del datagrama UDP**, no de un campo declarado por el remitente. El contenido recibido se trata exclusivamente como texto: se eliminan caracteres de control, se limita la longitud y **nunca se ejecuta**.

Esto permite usar el truco también como comprobación rápida de conectividad entre dos estaciones SST. Si ambos monitores descubren equipos pero no reciben sus mensajes, pueden estar interviniendo el firewall local, client isolation del Wi-Fi, filtrado de broadcast/VLAN o una política de red.

El canal es deliberadamente liviano y **no es autenticado ni cifrado**; sirve para señalización/diagnóstico local, no para transmitir secretos.

Si UDP/43837 no puede abrirse, el monitor normal continúa funcionando y muestra una advertencia de que los mensajes SST no están disponibles.

Se sale con `q` o `Esc`.

### Identificación puntual

```bash
net identify 10.11.24.20
net identify AE:C0:70:8F:4D:A2
net identify MI-CELULAR
net identify 10.11.24.20 --json
```

`net identify` acepta IP, MAC o nombre del inventario y muestra IP/hostname conocidos, MAC, scope, fabricante IEEE y registro (`MA-L`, `MA-M` o `MA-S`), nombre inventariado, método de descubrimiento, tiempo de respuesta y último avistamiento.

Para una MAC `local/private`, SST lo indica explícitamente y deja el fabricante sin atribuir, porque una dirección administrada localmente puede ser privada/aleatoria.

### Historial de presencia

```bash
net presence
net presence --json
net presence --csv
```

Consulta el historial persistente generado por `net monitor`.

### Tráfico por proceso

```bash
net traffic
net traffic --watch
net traffic --top 20
net traffic --pid 4120
net traffic --process chrome
net traffic --background
net traffic --high-usage
net traffic --unsigned
net traffic --connections
net traffic --json
net traffic --csv
```

Correlaciona:

- PID y PPID;
- nombre del proceso;
- proceso en primer plano;
- ejecutable;
- CPU;
- RAM;
- número de conexiones;
- subida y bajada por segundo.

La medición de bytes por PID utiliza ETW de Windows. Si ETW no está disponible, SST lo indica explícitamente en vez de inventar datos.

`--unsigned` conserva procesos cuyo estado Authenticode no sea `valid`; la columna `SIGNATURE` muestra `valid`, `unsigned`, otros estados normalizados o `unknown`.

`--connections` cambia la vista para mostrar protocolo, extremos local/remoto, estado, PID y proceso.

`--watch` abre una vista TUI actualizada periódicamente y se cierra con `q` o `Esc`.

Ejemplos:

```bash
net traffic --top 15
net traffic --process chrome --connections
net traffic --unsigned
net traffic --watch --background
```

### Proveedores de red

```bash
net provider list [--json]
net provider add NOMBRE --type TIPO --host HOST [--user-env VAR] [--secret-env VAR] [--community-env VAR]
net provider use NOMBRE
net provider current
net provider remove NOMBRE
net provider capabilities [NOMBRE]
net provider path
```

Tipos aceptados:

```text
openwrt
opnsense
pfsense
unifi
snmp
generic
```

Ejemplos:

```bash
net provider add gateway --type generic --host https://gateway.local/sst/usage
net provider add router --type openwrt --host https://router.local/sst/usage --user-env SST_ROUTER_USER --secret-env SST_ROUTER_SECRET
net provider use gateway
net provider current
net provider capabilities gateway
net provider list --json
```

Los perfiles guardan **nombres de variables**, no secretos. En el backend HTTP de `net usage`, `--user-env` + `--secret-env` produce autenticación básica; si solo existe `--secret-env`, se usa como token Bearer. `--community-env` queda disponible para integraciones SNMP.

### `net usage` — consumo de LAN desde un proveedor

```bash
net usage
net usage --top 10
net usage --device 192.168.1.10
net usage --mac AA:BB:CC:DD:EE:FF
net usage --json
net usage --csv
net usage --watch
```

`net usage` **no intenta deducir tráfico de otros equipos desde ARP, ping o la NIC local**. Consume contadores entregados por un router/firewall/controlador/endpoint que sí tenga visibilidad de la LAN.

Estado por tipo:

| Tipo | Estado actual |
|---|---|
| `generic` | HTTP JSON normalizado operativo. |
| `openwrt` | HTTP JSON normalizado operativo contra el endpoint configurado en `--host`. |
| `opnsense` | Requiere adapter/API específico; puede exponerse mediante un endpoint `generic` normalizado. |
| `pfsense` | Requiere adapter/API específico; puede exponerse mediante un endpoint `generic` normalizado. |
| `unifi` | Requiere adapter/controller API específico; puede exponerse mediante `generic`. |
| `snmp` | No se presenta como tráfico por cliente porque no existe un MIB universal para esa métrica. |

Contrato JSON aceptado:

```json
[
  {
    "device": "PC-01",
    "ip": "192.168.1.10",
    "mac": "AA:BB:CC:DD:EE:FF",
    "download_bps": 1250000,
    "upload_bps": 85000
  }
]
```

También acepta `{"clients":[...]}`, `hostname` como alias de `device`, `rx_bps` como alias de `download_bps` y `tx_bps` como alias de `upload_bps`.

`--watch` actualiza cada 2 segundos y actualmente se interrumpe con `Ctrl+C`.


---

## `device` — inventario local de equipos

```bash
device list
device list --json
device list --csv
device show MAC
device show NOMBRE
device add AA:BB:CC:DD:EE:FF NOMBRE
device add 10.11.24.20 NOMBRE
device add 10.11.24.20 NOMBRE --note "texto"
device remove AA:BB:CC:DD:EE:FF
device unknown
device unknown --json
device unknown --csv
device path
```

El inventario guarda MAC, nombre y notas. `device add` acepta una MAC directa o una IPv4 del mismo segmento; al recibir una IP, SST resuelve activamente la MAC mediante ARP antes de guardar el equipo.

`device list` y `device show` enriquecen el inventario con tipo de MAC, fabricante IEEE y registro MA-L/MA-M/MA-S cuando corresponde. `device show` añade además último IP, hostname y avistamiento disponibles en el historial de presencia.

Ejemplos:

```bash
device add 10.11.24.20 MI-CELULAR --note "Teléfono personal"
device show MI-CELULAR
device list
```

El inventario se utiliza para identificar dispositivos durante escaneos, Wake-on-LAN y localización en switches.

`device unknown` cruza el historial de presencia generado por `net monitor` con el inventario local y muestra únicamente los equipos detectados cuya MAC todavía no está registrada. La salida incluye IP, hostname, MAC, scope, fabricante, método de descubrimiento y última detección; también puede exportarse con `--json` o `--csv`.

No vuelve a escanear la red por su cuenta: trabaja sobre observaciones persistidas por SST, de modo que sirve para revisar posteriormente qué equipos aparecieron en una red administrada.

---

## `wol` — Wake-on-LAN

```bash
wol AA:BB:CC:DD:EE:FF
wol NOMBRE-INVENTARIADO
wol NOMBRE-INVENTARIADO 192.168.1.255:9
```

Envía un Magic Packet. El destino puede ser una MAC directa o el nombre de un equipo registrado con `device add`.

Si no se indica broadcast utiliza:

```text
255.255.255.255:9
```

---

## `domain` — estado de dominio

```bash
domain status
domain status EQUIPO
domain status EQUIPO --verify
domain status EQUIPO --cim
domain status EQUIPO --json
```

Para el equipo local consulta la pertenencia al dominio. Para equipos remotos puede obtener información básica y, con `--verify`/`--cim`, solicitar una comprobación más explícita.

La salida distingue:

- hostname;
- dominio;
- si está unido o no;
- fuente de la información;
- nivel de confianza;
- logon server.

---

## `switch` — MAC → switch → puerto físico

### Configuración

```bash
switch add SW-PISO2 --host 10.0.0.12 --community-env SW_CORE_COMMUNITY
switch add SW-PISO2 --host 10.0.0.12 --community-env SW_CORE_COMMUNITY --description "Core piso 2"
switch list
switch list --json
switch show SW-PISO2
switch remove SW-PISO2
switch path
switch capabilities
```

La comunidad SNMP **no se guarda en texto plano**. El perfil guarda el nombre de una variable de entorno:

```bash
export SW_CORE_COMMUNITY='comunidad-snmp'
```

### Localización

```bash
switch locate AA:BB:CC:DD:EE:FF
switch locate NOMBRE-INVENTARIADO
switch locate NOMBRE --switch SW-PISO2
switch locate NOMBRE --vlan 20
switch locate NOMBRE --json
```

SST consulta el switch por SNMP de solo lectura y resuelve, cuando el equipo lo permite:

```text
MAC
  ↓
switch
  ↓
bridge port
  ↓
ifIndex
  ↓
interfaz física
```

La salida puede incluir VLAN, PVID, alias, velocidad y estado operativo. La implementación utiliza Bridge-MIB, Q-BRIDGE-MIB e IF-MIB.

---

## `diag` — diagnósticos compuestos

```bash
diag network
diag dns
diag dns HOST
diag hardware
diag storage
diag traffic
diag domain
```

Agrupa varias herramientas para obtener una vista rápida:

- `diag network`: interfaces, vecinos y conexiones.
- `diag dns`: prueba resolución DNS; usa `example.com` si no se especifica destino.
- `diag hardware`: información general, memoria y discos.
- `diag storage`: estado de discos.
- `diag traffic`: tráfico de red por proceso.
- `diag domain`: pertenencia al dominio local.

---

# Utilidades Unix integradas

Estas herramientas están escritas o integradas dentro de SST y no requieren instalar GNU coreutils. La intención es ofrecer una experiencia familiar, no afirmar equivalencia completa con cada implementación GNU original.

| Comando | Función | Ejemplo |
|---|---|---|
| `pwd` | Directorio actual. | `pwd` |
| `echo` | Imprime argumentos. | `echo hola mundo` |
| `env` | Lista variables de entorno. | `env` |
| `clear` | Limpia la terminal. | `clear` |
| `ls` | Lista archivos; admite `-a` y `-l`. | `ls -la` |
| `cat` | Concatena archivo o stdin. | `cat notas.txt` |
| `head` | Primeras líneas. | `head notas.txt` |
| `tail` | Últimas líneas. | `tail notas.txt` |
| `grep` | Filtra líneas por texto/patrón soportado. | `grep error app.log` |
| `wc` | Cuenta líneas, palabras y bytes. | `wc notas.txt` |
| `sort` | Ordena líneas. | `sort nombres.txt` |
| `uniq` | Elimina líneas adyacentes repetidas. | `sort nombres.txt \| uniq` |
| `cut` | Selecciona campos. | `cut -d , -f 1 datos.csv` |
| `xargs` | Construye comandos desde stdin. | `printf "uno\\ndos\\n" \| xargs echo` |
| `tee` | Copia stdin a archivo y stdout. | `echo hola \| tee salida.txt` |
| `less` / `more` | Paginador interactivo. | `less app.log` |
| `sed` | Sustitución de texto soportada por SST. | `sed "s/error/ERROR/g" app.log` |
| `awk` | Selección simple de campos. | `awk '{print $1}' datos.txt` |
| `diff` | Compara dos archivos. | `diff antes.txt despues.txt` |
| `sha256sum` | SHA-256. | `sha256sum instalador.exe` |
| `base64` | Codifica/decodifica Base64. | `base64 archivo.txt` |
| `find` | Busca archivos. | `find .` |
| `printf` | Salida con formato. | `printf "%s\\n" hola` |
| `basename` | Último componente de ruta. | `basename /c/temp/a.txt` |
| `dirname` | Directorio de una ruta. | `dirname /c/temp/a.txt` |
| `realpath` | Ruta absoluta. | `realpath .` |
| `date` | Fecha/hora. | `date` |
| `sleep` | Espera un intervalo. | `sleep 2` |
| `true` | Estado 0. | `true` |
| `false` | Estado 1. | `false` |
| `touch` | Crea/actualiza archivo. | `touch notas.txt` |
| `mkdir` | Crea directorio. | `mkdir respaldo` |
| `rm` | Elimina archivo/directorio según opciones soportadas. | `rm archivo.tmp` |
| `cp` | Copia. | `cp origen.txt copia.txt` |
| `mv` | Mueve/renombra. | `mv viejo.txt nuevo.txt` |
| `tar` | Crea/lista/extrae TAR/TAR.GZ. | `tar -cf backup.tar carpeta` |
| `gzip` | Comprime a gzip. | `gzip datos.txt` |
| `gunzip` | Descomprime `.gz`. | `gunzip datos.txt.gz` |
| `zip` | Crea ZIP. | `zip backup.zip archivo.txt` |
| `unzip` | Lista/extrae ZIP. | `unzip backup.zip` |
| `which` | Localiza comando. | `which curl` |
| `type` | Indica resolución de comando. | `type net` |

## Programas externos disponibles desde SST

Además de sus herramientas integradas, SST puede lanzar ejecutables instalados en Windows o disponibles en `PATH`. Por ejemplo:

```bash
ipconfig /all
ping 8.8.8.8
netstat -ano
tasklist
systeminfo
powershell
ssh
curl
```

Estos programas **no forman parte de SST**; la shell simplemente los ejecuta como comandos externos.

---

# Configuración portable

El archivo principal es:

```text
config/sstrc
```

Usa sintaxis Bash-compatible y concentra tanto configuración de shell como apariencia de la terminal.

### Comandos

```bash
config path
config edit
config reload
config bg
config bg NOMBRE|NUMERO
config bg carrousel [MINUTOS]
config bg next
config bg off
```

- `config path`: muestra la ruta de `sstrc`.
- `config edit`: abre `sstrc` en Helix-SST.
- `config reload`: recarga la configuración sin reiniciar SST.
- `config bg`: lista las imágenes de la carpeta portable `bg/` y muestra el modo actual.
- `config bg NOMBRE|NUMERO`: selecciona un fondo fijo por nombre, stem o número de la lista.
- `config bg carrousel`: activa el carrusel con intervalo de 3 minutos.
- `config bg carrousel 5`: cambia el intervalo a 5 minutos; se aceptan valores entre 1 y 60.
- `config bg next`: avanza manualmente al fondo siguiente y lo deja fijo.
- `config bg off`: desactiva la imagen de fondo.

El builtin interno equivalente es `sst-config`; `config` es la interfaz normal del shell.

### Carpeta `bg/`

SST crea automáticamente:

```text
bg/
```

junto a `sst.exe`. Formatos admitidos:

```text
PNG  JPG/JPEG  WebP  BMP  GIF  ICO  TIFF
```

Ejemplo:

```text
sst.exe
bg/
  README.txt
  amber-city.jpg
  gruvbox-terminal.png
  observatory.webp
config/
data/
```

Entonces:

```bash
config bg
config bg gruvbox-terminal
config bg carrousel
```

Los cambios hechos con `config bg` se aplican a la ventana actual; no requieren reiniciar SST.

### Carrusel sensible al estado

En `carrousel`, el fondo cambia:

- al cumplirse el intervalo configurado;
- al entrar o salir de raw/alternate screen, lo que cubre Helix-SST, `net monitor`, `sys top` y otras TUIs;
- al minimizar o restaurar la ventana;
- al maximizar o restaurar la ventana.

La rotación ocurre dentro del renderer; no reescribe `sstrc` en cada cambio.

Variables persistentes:

```bash
SST_BACKGROUND_MODE='off'              # off | fixed | carrousel
SST_BACKGROUND_IMAGE=''
SST_BACKGROUND_CAROUSEL_MINUTES=3
SST_BACKGROUND_IMAGE_OPACITY=100
SST_BACKGROUND_IMAGE_FIT='cover'       # cover | contain | fill | preserve
```

### Apariencia

La apariencia se configura también en `config/sstrc`:

```bash
SST_BACKDROP='acrylic'
SST_FOCUSED_OPACITY=80
SST_UNFOCUSED_OPACITY=0
SST_BACKGROUND_COLOR='#111629'
SST_CORNER_RADIUS=16
SST_CONTENT_TOP_GAP=12
SST_FONT_SIZE=13
SST_CELL_WIDTH=8
SST_CELL_HEIGHT=17
SST_TERMINAL_PADDING_X=8
SST_TERMINAL_PADDING_Y=6
```

Valores de `SST_BACKDROP`:

- `acrylic`: desenfoque/tinte fuerte;
- `blur`: desenfoque simple;
- `glass`: cristal DWM clásico;
- `solid`: fondo opaco.

Las instalaciones antiguas que todavía tengan `config/terminal.toml` se migran a `sstrc`; después ese archivo legado se elimina.

La variable que contiene la ruta de la configuración Bash es:

```text
SST_CONFIG
```

`config/sstrc` también puede contener las credenciales de `intel` mediante `export`:

```bash
export SST_MALWAREBAZAAR_AUTH_KEY='...'
export SST_THREATFOX_AUTH_KEY='...'
export SST_URLHAUS_AUTH_KEY='...'
```

---

# Traducción de rutas

```bash
sst-path /c/Windows/System32
```

Convierte rutas estilo Unix a rutas Windows.

SST también muestra rutas del prompt de forma familiar:

```text
C:\Users\usuario       → ~
C:\Windows\System32    → /c/Windows/System32
```

---

# Editor integrado: helix-sst

Comandos registrados:

```bash
helix archivo.txt
hx archivo.txt
helix-sst archivo.txt
helix --version
helix --credits
helix --help
```

La distribución portable está basada en **Helix 25.07.1**; la integración **Helix-SST 0.2.1** aporta:

- empaquetado dentro de SST;
- configuración portable;
- **tema Gruvbox Dark**;
- puente PTY/ConPTY y transporte VT;
- integración de clipboard;
- corrector ortográfico **offline es-CL** para `.txt` y Markdown;
- diagnósticos ortográficos y sugerencias mediante `F2`;
- `Alt+d` en modo insertar para escribir el guion largo `—`; `Ctrl+g` queda como atajo alternativo;
- integración con la terminal nativa.

### Estado actual

**helix-sst está funcional dentro de SST.**

El editor puede iniciarse y utilizarse de forma interactiva desde la terminal nativa, incluyendo edición mediante teclado, navegación y guardado a través del puente PTY/ConPTY integrado.

Ejemplo para escritura:

```bash
helix capitulo.txt
```

Dentro de Helix-SST:

- `i`: modo insertar;
- `Alt+d`: inserta `—`;
- `Ctrl+g`: atajo alternativo para `—`;
- `F2`: muestra correcciones disponibles para el diagnóstico bajo el cursor;
- los errores ortográficos se muestran en el gutter y al final de la línea;
- `F1`: guía integrada;
- `Ctrl+V`: pegado;
- `:w`: guardar;
- `:q`: salir.

El corrector no envía el texto a servicios externos.

El log queda en:

```text
config/helix-sst/helix.log
```

---

# Ejemplos de scripting

SST **despliega automáticamente** `tour.sh` y el contenido de `examples/` junto a `sst.exe` al arrancar si faltan. Esto permite que una distribución portable que contenga solo el binario reconstruya los ejemplos localmente.

La carpeta `examples/` contiene demostraciones ejecutables que muestran la consola como entorno de automatización, no solo como lanzador de comandos.

```text
examples/
  01-language-tour.sh
  02-windows-operator-report.sh
  03-network-discovery.sh
  04-jobs-and-coproc.sh
  05-pipelines-and-text.sh
  06-security-triage.sh
  07-operator-console.sh
  08-config-and-backgrounds.sh
  09-advanced-bash.sh
  10-full-showcase.sh
```

Se ejecutan directamente desde SST:

```bash
examples/01-language-tour.sh
examples/02-windows-operator-report.sh
examples/03-network-discovery.sh 10.11.24.0/24
examples/04-jobs-and-coproc.sh
examples/07-operator-console.sh
examples/09-advanced-bash.sh
examples/10-full-showcase.sh 10.11.24.0/24
```

Los ejemplos cubren, entre otras capacidades:

- funciones, variables locales y parámetros;
- arrays indexados y asociativos;
- `if`, `case`, `for`, bucles aritméticos y `select`;
- `[[ ... ]]` y aritmética `(( ... ))`;
- command substitution;
- here-docs;
- pipelines y `pipefail`;
- redirecciones sobre grupos;
- traps y limpieza temporal;
- jobs en background, `$!`, `jobs`, `wait -n -p`;
- coprocesos con `coproc`, arrays de descriptores y `read -u`;
- process substitution con `<( ... )`;
- utilidades Unix integradas;
- composición con `sys`, `net`, `device`, `eventlog`, `service`, `pnp`, `firewall`, `triage` e `intel`;
- menús interactivos construidos enteramente dentro de Nwash;
- configuración portable y fondos dinámicos;
- `getopts`, namerefs, arrays dispersos, `mapfile`, `printf -v` y stack de funciones.

Los scripts incluidos son no destructivos por diseño. El objetivo es que funcionen como **showcase técnico, material de aprendizaje y base para automatizaciones reales**.

La guía detallada está en `examples/README.md`. Para una demostración de punta a punta, `examples/10-full-showcase.sh` combina sistema, Windows, red, jobs, ETW, triage, Intel y generación de reportes en una sola ejecución.

---

## SST Tour interactivo

Escribe simplemente:

```bash
tour
```

SST abre una TUI:

```text
SST TOUR
────────────────────────────────────────────────────────────
↑/↓ seleccionar · Enter ejecutar · Home/End · q/Esc salir

▶ 01 · Lenguaje Nwash          funciones, arrays, case, aritmética
  02 · Operador Windows        reporte de sistema y administración
  03 · Descubrimiento de red   scan, inventario, snapshots, diff
  ...
  10 · Full showcase           SST completo de punta a punta
```

El selector interno usa teclas nativas de la terminal, por lo que las flechas funcionan sin depender de secuencias ANSI interpretadas por un script. Al pulsar `Enter`, `tour.sh` ejecuta el ejemplo seleccionado dentro del propio intérprete Nwash. Al terminar, vuelve al menú.

Archivos desplegados:

```text
sst.exe
tour.sh
examples/
  README.md
  01-language-tour.sh
  ...
  10-full-showcase.sh
```

---

# Estado de Bash

SST **no ejecuta un Bash externo**. Tiene un intérprete propio escrito en Rust con objetivo de compatibilidad con **Bash 5.3**.

No debe confundirse “compatible con Bash” con “GNU Bash recompilado para Windows”: SST implementa la sintaxis y semántica dentro de su propio motor y adapta al modelo de procesos y archivos de Windows las partes que dependen de Unix.

## Lenguaje implementado

Actualmente están implementados:

- comandos simples y secuencias;
- `&&`, `||` y `!`;
- pipes `|` y `|&`;
- pipelines paralelos;
- ejecución en background con `&`;
- grupos `{ ...; }`;
- subshells `(...)`;
- `if / elif / else / fi`;
- `for`;
- `for ((...))`;
- `while`;
- `until`;
- `select`;
- `case` con `;;`, `;&` y `;;&`;
- funciones;
- `[[ ... ]]`;
- `(( ... ))`;
- `time`;
- `coproc`;
- asignaciones simples y arrays;
- variables locales y scopes;
- parámetros posicionales `$0`, `$1`, `$@`, `$*`, `$#`;
- arrays indexados;
- arrays asociativos;
- namerefs;
- arrays dispersos;
- atributos de variables;
- funciones exportadas.

## Expansiones

Incluye:

- expansión de variables;
- valores por defecto y operadores de parámetros;
- sustitución de comandos `$(...)`;
- expansión aritmética `$((...))`;
- brace expansion;
- tilde expansion;
- globbing;
- `extglob`;
- `globstar`;
- `GLOBIGNORE`;
- `GLOBSORT`;
- `dotglob`;
- `nullglob`;
- `failglob`;
- `nocaseglob`;
- IFS;
- quoting simple y doble;
- quoting ANSI-C;
- sustituciones modernas de Bash 5.3 ejecutadas en el shell actual.

## Redirecciones

Se implementan:

- `>`;
- `>>`;
- `<`;
- `<>`;
- `2>` y otros descriptores;
- duplicación `2>&1`;
- cierre de descriptores;
- `&>`;
- `&>>`;
- heredocs;
- here-strings;
- descriptores asignados a variables, por ejemplo `{fd}>archivo`;
- redirecciones sobre comandos compuestos.

## Process substitution y coprocesos

```bash
diff <(comando1) <(comando2)
coproc mi_proceso { comando; }
```

SST implementa process substitution y coprocesos usando mecanismos compatibles con Windows.

## Job control

Implementado:

```text
jobs
fg
bg
wait
wait -n
disown
kill
```

También reconoce job specs como `%1`, `%+`, `%-` y búsquedas por nombre.

El control de jobs está adaptado a procesos y threads de Windows, por lo que no existe un controlling TTY POSIX idéntico al de Linux.

## Scripts `.sh`

SST reconoce scripts Bash/sh:

```bash
test.sh
./test.sh
sst.exe test.sh
```

En el estado actual de `main`, un script local Bash/sh puede ejecutarse directamente dentro del intérprete activo. Esto evita relanzar otra instancia de SST y permite que `read` utilice el transporte de entrada de la terminal actual.

SST acepta deliberadamente un `.sh` del directorio actual por nombre, por ejemplo `test.sh`.

## Builtins Bash disponibles

```text
:
.
[
alias
bg
bind
break
builtin
caller
cd
command
compgen
complete
compopt
continue
declare
dirs
disown
echo
enable
eval
exec
exit
export
false
fc
fg
getopts
hash
help
history
jobs
kill
let
local
logout
mapfile
popd
printf
pushd
pwd
read
readarray
readonly
return
set
shift
shopt
source
suspend
test
times
trap
true
type
typeset
ulimit
umask
unalias
unset
wait
```

Entre las funciones relevantes se encuentran:

- `read -e` y `read -E`;
- `source -p`;
- `trap -P`;
- `compgen -V`;
- programmable completion con `complete`, `compgen` y `compopt`;
- historial con `history` y `fc`;
- bindings con `bind`;
- `mapfile`/`readarray`;
- `getopts`;
- `hash`;
- pila de directorios con `dirs`, `pushd`, `popd`.

## Variables especiales

SST implementa, entre otras:

```text
BASH_VERSION
BASH_VERSINFO
BASHPID
PPID
BASH_SUBSHELL
BASH_ARGC
BASH_ARGV
BASH_ARGV0
BASH_COMMAND
BASH_SOURCE
BASH_LINENO
FUNCNAME
PIPESTATUS
BASH_ALIASES
BASH_CMDS
SHELLOPTS
BASHOPTS
RANDOM
SRANDOM
SECONDS
EPOCHSECONDS
EPOCHREALTIME
BASH_MONOSECONDS
```

## Opciones `set -o`

Reconocidas actualmente:

```text
allexport
braceexpand
emacs
errexit
errtrace
functrace
hashall
histexpand
history
ignoreeof
interactive-comments
keyword
monitor
noclobber
noexec
noglob
nolog
notify
nounset
onecmd
physical
pipefail
posix
privileged
verbose
vi
xtrace
```

## Opciones `shopt`

Reconocidas actualmente:

```text
array_expand_once
assoc_expand_once
autocd
bash_source_fullpath
cdable_vars
cdspell
checkhash
checkjobs
checkwinsize
cmdhist
compat31
compat32
compat40
compat41
compat42
compat43
compat44
compat50
compat51
compat52
compat53
complete_fullquote
direxpand
dirspell
dotglob
execfail
expand_aliases
extdebug
extglob
extquote
failglob
force_fignore
globasciiranges
globskipdots
globstar
gnu_errfmt
histappend
histreedit
histverify
hostcomplete
huponexit
inherit_errexit
interactive_comments
lastpipe
lithist
localvar_inherit
localvar_unset
login_shell
mailwarn
no_empty_cmd_completion
nocaseglob
nocasematch
noexpand_translation
nullglob
patsub_replacement
progcomp
progcomp_alias
promptvars
restricted_shell
shift_verbose
sourcepath
varredir_close
xpg_echo
```

## Estado real de compatibilidad Bash 5.3

La cobertura es amplia, pero **todavía no debe declararse equivalencia total con GNU Bash 5.3**.

Pendientes o diferencias conocidas:

| Área | Estado |
|---|---|
| `enable -f` / `enable -d` | No hay sistema de builtins cargables dinámicamente. |
| GNU Readline completo | SST implementa edición, history, completion y bindings propios, pero no replica toda GNU Readline 8.3. |
| `/dev/tcp/HOST/PORT` | No implementado actualmente. |
| `/dev/udp/HOST/PORT` | No implementado actualmente. |
| `test -u`, `test -g`, `test -k` | No tienen equivalente directo en NTFS y actualmente devuelven falso. |
| `test -O`, `test -G` | Aproximados; Windows no usa el modelo POSIX de propietario/grupo. |
| `test -x` | Aproximado al modelo de archivos de Windows. |
| `umask` | SST mantiene el valor lógico, pero Windows no aplica permisos POSIX al crear archivos. |
| `ulimit` | Mantiene los límites dentro del estado de la shell; no impone límites POSIX al proceso Windows. |
| `suspend` | Emulación interactiva, no suspensión POSIX real de la shell. |
| señales | Traducidas/adaptadas a procesos Windows; no existe equivalencia completa con las señales Unix. |
| `disown -h` | No reproduce literalmente la semántica SIGHUP de Unix. |
| controlling TTY | Windows/ConPTY no ofrece el mismo modelo de controlling terminal POSIX. |
| conformidad exhaustiva | Existe una suite comparativa básica, pero todavía no se ha certificado toda la semántica contra GNU Bash 5.3. |

---

# Terminal nativa

La interfaz gráfica de SST es una terminal Win32 propia e incluye:

- barra de título personalizada;
- controles propios de minimizar, maximizar y cerrar;
- backdrop/transparencia en Windows compatible;
- renderer VT;
- scrollback;
- selección con mouse;
- Nerd Font privada embebida;
- historial;
- autocompletado;
- soporte para TUIs;
- clipboard.

Interacción relevante:

- arrastrar con el mouse selecciona texto;
- doble clic selecciona palabra/token;
- triple clic selecciona la línea;
- `Ctrl+C`: copia cuando existe una selección; sin selección conserva el uso de interrupción;
- `Ctrl+Shift+C` y `Ctrl+Insert`: copiar;
- `Ctrl+V`, `Ctrl+Shift+V` y `Shift+Insert`: pegar;
- clic derecho: copia si hay selección; si no la hay, pega.

---

# Datos portables

SST crea junto al ejecutable:

```text
config/
data/
bg/
```

Entre los datos persistentes se encuentran:

- `config/sstrc`: configuración de la shell;
- inventario de dispositivos;
- historial de presencia;
- perfiles de proveedores de red;
- perfiles de switches;
- historial de comandos;
- configuración y log de helix-sst.

---

# Estado general del proyecto

### Funcional

- terminal Win32 propia;
- intérprete Bash nativo con cobertura amplia de Bash 5.3;
- ejecución de scripts `.sh`;
- utilidades Unix incluidas;
- información y auditoría local de Windows;
- diagnóstico de red;
- escaneo multi-evidencia ARP/ICMP/TCP y monitor de presencia con histéresis;
- mensajes UDP broadcast entre instancias de `net monitor` para señalización y prueba de comunicación LAN;
- clasificación de MAC global/local/multicast/broadcast e identificación IEEE MA-L/MA-M/MA-S;
- `net identify` por IP/MAC/nombre;
- inventario de dispositivos con alta directa por IP;
- Wake-on-LAN;
- dominio;
- localización MAC → switch → puerto mediante SNMP;
- tráfico por proceso mediante ETW y estado Authenticode;
- `net usage` mediante endpoint HTTP JSON normalizado para proveedores `generic`/`openwrt`;
- Helix-SST 0.2.1 con Gruvbox Dark y corrector es-CL offline;
- configuración portable;
- fondos fijos y carrusel de imágenes sensible al estado de la terminal;
- colección `examples/` de scripts de demostración avanzada.

### Pendiente o parcial

- adapters específicos de `net usage` para OPNsense/pfSense/UniFi y MIBs SNMP concretos;
- equivalencia completa con GNU Readline;
- builtins dinámicos de Bash;
- `/dev/tcp` y `/dev/udp`;
- semántica Unix que no tiene equivalente directo en Windows;
- suite exhaustiva de conformidad Bash.

El documento de alcance y arquitectura está en:

```text
SHELL_SHOCK_TOOL_PLAN.md
```
