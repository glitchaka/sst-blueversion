# Shell Shock Tool (SST) — Manual de comandos

> Manual de uso basado en el estado actual de `main` de `glitchaka/sst-blueversion`.
>
> Este documento describe lo que está implementado actualmente. Cuando una capacidad aparece registrada pero todavía no está conectada por completo, se indica expresamente.

## Convenciones usadas en este manual

- `PID`: identificador numérico de proceso.
- `PPID`: PID del proceso padre.
- `NOMBRE`: nombre lógico de un objeto, por ejemplo una impresora, servicio o dispositivo.
- `HOST`: hostname o dirección IP.
- `MAC`: dirección MAC, por ejemplo `AA:BB:CC:DD:EE:FF`.
- `RED`: red IPv4 en formato CIDR, por ejemplo `10.11.24.0/24`.
- `RUTA`: ruta de archivo o directorio.
- `CLAVE`: clave de Registro de Windows.
- Los parámetros entre corchetes `[...]` son opcionales.
- Muchas operaciones de lectura funcionan sin elevación. Las operaciones que modifican Windows pueden requerir `sudo`.
- Para ver la ayuda disponible desde SST, usa `help`, `COMANDO --help` o el subcomando `help` cuando corresponda.

---

# 1. Sistema — `sys`

`sys` agrupa información local del equipo, procesos, memoria, discos y varias herramientas de administración de Windows.

## `sys info`

```bash
sys info
```

Muestra un resumen del equipo:

- hostname;
- versión de Windows;
- versión del kernel;
- uptime;
- cantidad de CPU lógicas;
- RAM total;
- RAM usada.

Úsalo como primera vista rápida del equipo.

---

## `sys processes`

```bash
sys processes
```

Muestra hasta 160 procesos en forma de árbol con:

- PID;
- PPID;
- CPU;
- RAM;
- nombre del proceso;
- relación jerárquica padre/hijo.

Alias:

```bash
ps
```

Es útil para localizar procesos consumidores o confirmar quién creó a quién antes de usar `sys inspect`, `sys why` o `triage PID`.

---

## `sys top`

```bash
sys top
```

Abre un monitor interactivo de procesos.

Controles:

```text
c    ordenar por CPU
m    ordenar por memoria
q    salir
Esc  salir
```

La cabecera muestra uptime, cantidad de CPU y memoria usada/total.

Alias:

```bash
top
```

---

## `sys disks`

```bash
sys disks
```

Muestra por unidad o punto de montaje:

- capacidad total;
- espacio usado;
- espacio libre;
- porcentaje de uso;
- sistema de archivos.

Alias:

```bash
df
```

---

## `sys memory`

```bash
sys memory
```

Muestra memoria física y swap:

```text
total
used
free
```

Alias:

```bash
free
```

Para un análisis orientado a presión de memoria y principales consumidores usa además:

```bash
triage --memory
```

---

## `sys uptime`

```bash
sys uptime
```

Muestra cuánto tiempo lleva iniciado Windows, tanto en formato legible como en segundos.

Alias:

```bash
uptime
```

---

## `sys hostname`

```bash
sys hostname
```

Devuelve el nombre local del equipo.

Alias:

```bash
hostname
```

---

## `sys whoami`

```bash
sys whoami
```

Muestra el usuario actual. Si hay dominio disponible, el resultado adopta la forma:

```text
dominio\usuario
```

Alias:

```bash
whoami
```

---

## `sys uname`

```bash
sys uname
sys uname -a
```

Sin opciones devuelve el identificador SST:

```text
SST-Windows
```

Con `-a` añade hostname, kernel, arquitectura y sistema operativo.

Alias:

```bash
uname
```

---

## `sys kill`

```bash
sys kill PID
sys kill PID --tree
sys kill NOMBRE
sys kill NOMBRE --tree
```

Termina procesos usando la API Win32.

### Por PID

```bash
sys kill 14508
```

Termina exactamente ese proceso.

```bash
sys kill 14508 --tree
```

Termina primero los descendientes y después el PID raíz.

### Por nombre

```bash
sys kill msedge.exe
```

Intenta terminar todos los procesos con ese nombre.

```bash
sys kill msedge.exe --tree
```

Incluye descendientes y vuelve a revisar varias veces por si la aplicación crea procesos hermanos o reaparece durante el cierre.

También acepta `-t` como equivalente de `--tree`.

> Algunos procesos protegidos pueden requerir elevación.

Alias Bash/SST:

```bash
kill
```

El builtin Bash `kill` además entiende señales y jobs, por lo que no es exactamente la misma interfaz que `sys kill`.

---

## `sys services`

```bash
sys services
sys services --running
sys services --stopped
sys services NOMBRE
sys services --impact
```

### Listado

```bash
sys services
```

Consulta todos los servicios mediante `sc.exe`.

### Filtrado

```bash
sys services --running
sys services --stopped
```

Muestra sólo activos o detenidos.

### Servicio concreto

```bash
sys services Spooler
```

Consulta el estado de un servicio.

### Impacto

```bash
sys services --impact
```

Correlaciona servicios con procesos y muestra consumo observado de CPU y RAM.

`sys services` está pensado principalmente para consulta. Para iniciar, detener o reiniciar servicios usa la herramienta modificable:

```bash
service
```

---

## `sys users`

```bash
sys users
sys users USUARIO
sys users --domain
sys users USUARIO --domain
```

Usa las herramientas nativas de Windows para consultar cuentas.

Ejemplos:

```bash
sys users
sys users manuel
sys users --domain
```

---

## `sys printers`

```bash
sys printers
sys printers --default
sys printers NOMBRE
```

### Todas las impresoras

```bash
sys printers
```

Muestra:

- nombre de cola;
- servidor;
- recurso compartido;
- puerto;
- IP cuando puede resolverse.

La impresora predeterminada aparece marcada con `*`.

### Sólo la predeterminada

```bash
sys printers --default
```

### Una impresora concreta

```bash
sys printers Provi60_A-2_informatica
```

Cuando se entrega un nombre, la orden delega en la misma búsqueda detallada de `sys printer` para no caer al listado general.

---

## `sys printer`

```bash
sys printer
sys printer NOMBRE
sys printer NOMBRE --ip
```

### Sin nombre

```bash
sys printer
```

Muestra la impresora predeterminada.

### Por nombre o recurso compartido

```bash
sys printer Provi60_A-2_informatica
```

Muestra:

- nombre;
- si es predeterminada;
- servidor;
- nombre compartido;
- puerto;
- IP;
- driver;
- ubicación.

También puede localizar una impresora por su nombre UNC o por la parte compartida.

### Sólo datos de red

```bash
sys printer Provi60_A-2_informatica --ip
```

Devuelve servidor, puerto e IP.

---

## `sys drivers`

```bash
sys drivers
sys drivers --verbose
sys drivers --signed
sys drivers --csv
sys drivers --pnp
sys drivers --devices
```

### Drivers instalados

```bash
sys drivers
```

Usa `driverquery.exe`.

### Detalle

```bash
sys drivers --verbose
```

### Información de firma

```bash
sys drivers --signed
```

### CSV

```bash
sys drivers --csv
```

La salida se puede redirigir:

```bash
sys drivers --verbose --csv > drivers.csv
```

### Paquetes PnP

```bash
sys drivers --pnp
```

Usa `pnputil /enum-drivers`.

### Dispositivos conectados

```bash
sys drivers --devices
```

Usa `pnputil /enum-devices /connected`.

---

## `sys events`

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

### Consulta normal

```bash
sys events
```

Lee los últimos 20 eventos de `System`.

```bash
sys events Application
```

Cambia el log.

### Cantidad

```bash
sys events --count 50
```

Acepta de 1 a 500 eventos.

### XPath

```bash
sys events --log System --query "*[System[(Level=2)]]"
```

Pasa el filtro a `wevtutil`.

### Formato

```bash
sys events --format text
sys events --format xml
```

### Descubrir logs y publishers

```bash
sys events --logs
sys events --publishers
```

---

## `sys registry` / `sys reg`

Interfaz de **sólo lectura** para el Registro.

```bash
sys registry CLAVE
sys registry query CLAVE
sys registry CLAVE --value NOMBRE
sys registry CLAVE --default
sys registry CLAVE --recursive
sys registry CLAVE --find TEXTO
sys registry CLAVE --find TEXTO --keys
sys registry CLAVE --find TEXTO --data
```

Ejemplo:

```bash
sys registry "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
```

Para modificar el Registro usa la herramienta `registry`, no `sys registry`.

---

## `sys tasks` / `sys scheduled-tasks`

```bash
sys tasks
sys tasks --verbose
sys tasks --csv
sys tasks NOMBRE
sys tasks NOMBRE --xml
```

### Listado

```bash
sys tasks
```

### Detallado

```bash
sys tasks --verbose
```

### CSV

```bash
sys tasks --csv
```

### Tarea concreta

```bash
sys tasks "\Microsoft\Windows\Defrag\ScheduledDefrag"
```

### XML

```bash
sys tasks "\Microsoft\Windows\Defrag\ScheduledDefrag" --xml
```

Para ejecutar, detener, habilitar, deshabilitar o eliminar tareas usa `task`.

---

# 1.1 Seguridad integrada en `sys`

## `sys inspect PID`

```bash
sys inspect PID
sys inspect PID --deep
```

Inspecciona un proceso y muestra:

- PID/PPID;
- nombre;
- inicio;
- CPU;
- RAM;
- ruta;
- línea de comandos;
- muestra de I/O.

Ejemplo:

```bash
sys inspect 8788
```

Con:

```bash
sys inspect 8788 --deep
```

añade información de linaje/hijos disponible para ese proceso.

---

## `sys why PID`

```bash
sys why PID
```

Explica por qué el motor de correlación clasificó ese proceso como normal, de atención o sospechoso.

Puede incluir:

- relación con su padre;
- historial local;
- relación aprendida;
- patrones de ejecución;
- información que todavía no está disponible para esa evaluación.

Ejemplo:

```bash
sys why 8788
```

---

## `sys diff PID`

```bash
sys diff PID
```

Compara el proceso actual con su historial local:

- padre actual frente a padres observados;
- relaciones aprendidas;
- política de confianza manual;
- cambios de línea de comandos cuando hay datos previos.

Es especialmente útil después de `sys why`.

---

## `sys suspicious`

```bash
sys suspicious
```

Vuelve a tomar una muestra de procesos y muestra únicamente los que superan las reglas actuales de atención.

No equivale a afirmar que un proceso sea malware: es una lista de candidatos a revisar.

---

## `sys safe`

```bash
sys safe PID
sys safe PID PID ...
sys safe list
sys safe remove ID
```

Permite registrar manualmente una relación padre/hijo como confiable.

### Registrar

```bash
sys safe 8788
```

SST obtiene el ejecutable del proceso y el ejecutable de su padre y guarda esa relación.

### Ver reglas

```bash
sys safe list
```

### Eliminar

```bash
sys safe remove 3
```

El número corresponde al ID mostrado por `sys safe list`.

> La confianza manual sólo afecta esa relación de linaje. No debería impedir que otras señales actuales vuelvan a elevar el proceso.

---

## `sys startup`

```bash
sys startup
```

Revisa puntos de inicio conocidos, incluyendo claves `Run`/`RunOnce` de HKCU/HKLM y carpetas de inicio.

---

## `sys persistence`

```bash
sys persistence
```

Agrupa la vista de inicio automático y recuerda otros puntos que deben revisarse, como tareas y servicios.

---

## Broker de procesos

```bash
sys inspect PID --broker
sys suspend PID --start-time FILETIME
sys resume PID --start-time FILETIME
sys kill PID --broker --start-time FILETIME
```

Estas variantes usan el broker autenticado del proyecto para operaciones sensibles sobre procesos.

`--start-time FILETIME` ayuda a comprobar la identidad de la instancia concreta del PID y evita actuar sobre un PID reutilizado.

---

# 1.2 Alias directos del sistema

```text
ps         -> procesos
top        -> monitor TUI
df         -> discos
free       -> memoria
hostname   -> hostname
whoami     -> usuario
uname      -> identificación del sistema
uptime     -> uptime
fetch      -> resumen visual SST
neofetch   -> alias de fetch
fastfetch  -> alias de fetch
```

## `fetch`, `neofetch`, `fastfetch`

```bash
fetch
fetch --small
fetch --full
fetch --plain
fetch --stdout
```

Muestra logo SST e información de:

- usuario/host;
- Windows;
- kernel;
- uptime;
- shell;
- versión SST;
- terminal;
- CPU;
- RAM;
- arquitectura.

Para guardar a archivo sin secuencias ANSI:

```bash
neofetch --stdout > neofetch.txt
```

o:

```bash
neofetch --plain > neofetch.txt
```

---

# 2. Triage de seguridad — `triage`

`triage` es la herramienta de correlación activa de SST. No se limita a mostrar el resultado que apareció al abrir la consola.

## `triage`

```bash
triage
```

Toma una muestra nueva y revisa:

- procesos actuales;
- rutas conocidas y nuevas;
- relaciones padre/hijo;
- historial local;
- relaciones aprendidas;
- hallazgos de seguridad;
- consumo relevante de recursos.

Si la presión de RAM supera el umbral configurado en la implementación, también añade diagnóstico de memoria.

---

## `triage --scan`

```bash
triage --scan
```

Relanza el escaneo de inicio completo, incluyendo la salida tipo:

```text
[·] procesos
[✓] árbol PID/PPID
[✓] CPU / RAM / I/O
[✓] historial local
...
«Hola. ¿Te gustaría destruir algún mal hoy?»
```

Alias:

```bash
triage --refresh
```

Este es el comando adecuado para repetir manualmente el análisis que SST ejecuta al iniciar.

---

## `triage --memory`

```bash
triage --memory
```

Se concentra en presión de memoria y principales consumidores.

Alias:

```bash
triage --ram
```

Muestra:

- RAM total;
- RAM usada;
- RAM disponible;
- porcentaje de presión;
- suma orientativa de memoria visible en procesos;
- top de procesos por RAM.

La suma de los procesos no debe interpretarse como igualdad exacta con la RAM total usada por Windows: también existen kernel, caché, memoria comprimida, pools, drivers y páginas compartidas.

---

## `triage --deep`

```bash
triage --deep
```

Hace el análisis general y añade correlaciones más costosas:

- memoria;
- conexiones asociadas a hallazgos;
- inicio automático;
- impacto de servicios.

---

## `triage PID`

```bash
triage 8788
```

Agrupa en una sola investigación:

1. `sys why PID`;
2. `sys inspect PID`;
3. `sys diff PID`.

Es la forma rápida de pasar de “este proceso merece revisión” a “qué es y por qué apareció”.

---

## `triage PID --deep`

```bash
triage 8788 --deep
```

Añade las conexiones TCP/UDP observadas para ese PID.

---

## Aprendizaje local

SST mantiene historial de relaciones padre/hijo. Para procesos instalados en rutas de sistema o `Program Files`, una relación observada repetidamente puede pasar a formar parte del baseline local.

En WebView2 se normalizan rutas versionadas para evitar que cada actualización de una aplicación de Microsoft Store parezca una identidad completamente distinta.

El aprendizaje reduce la **novedad de linaje**, pero no convierte el proceso en permanentemente seguro. Otras señales pueden volver a elevarlo.

---

# 3. Inteligencia — `intel`

## `intel status`

```bash
intel status
```

Muestra el registro de fuentes de inteligencia y para cada una:

- estado habilitado/deshabilitado;
- adaptador;
- TTL de caché;
- disponibilidad de autenticación.

---

## `intel sources`

```bash
intel sources
```

Muestra la configuración registrada de las fuentes y la ruta del archivo de configuración.

Fuentes configuradas en el proyecto:

```text
MalwareBazaar
ThreatFox
URLhaus
LOLBAS
```

---

## `intel update`

```bash
intel update
```

Elimina entradas de caché expiradas y muestra la capacidad de cada fuente.

No equivale necesariamente a descargar una base completa; prepara/actualiza el estado de las fuentes y caché según el adaptador implementado.

---

## `intel lookup`

```bash
intel lookup INDICADOR
```

Consulta un indicador.

Ejemplos conceptuales:

```bash
intel lookup <SHA256>
intel lookup dominio.example
intel lookup 203.0.113.5
```

SST comprueba primero sus listas locales y después las fuentes habilitadas compatibles, usando caché cuando existe una respuesta vigente.

---

# 4. Red — `net`

## `net interfaces`

```bash
net interfaces
```

Muestra las interfaces de red locales.

Úsalo para identificar:

- adaptador Ethernet/Wi-Fi;
- IP;
- configuración relevante del equipo antes de hacer diagnóstico.

---

## `net connections`

```bash
net connections
```

Muestra conexiones TCP/UDP con:

- protocolo;
- dirección local;
- dirección remota;
- estado;
- PID propietario.

Para una vista centrada en procesos y tráfico usa también:

```bash
net traffic --connections
```

---

## `net routes`

```bash
net routes
```

Muestra la tabla de rutas del equipo.

---

## `net dns`

```bash
net dns HOST
net dns IP
```

### Directa

```bash
net dns servidor.midominio.local
```

Resuelve hostname a una o más direcciones.

### Inversa

```bash
net dns 10.6.21.3
```

Intenta obtener el nombre asociado a la IP.

---

## `net ping`

```bash
net ping HOST
net ping HOST -c N
```

Por defecto realiza 4 intentos.

Ejemplo:

```bash
net ping 10.6.21.3 -c 10
```

`N` debe estar entre 1 y 100.

La salida incluye latencia por respuesta y resumen enviados/recibidos.

---

## `net trace` / `net traceroute`

```bash
net trace HOST
net traceroute HOST
```

Realiza un trazado ICMP de hasta 30 saltos.

---

## `net neighbors` / `net arp`

```bash
net neighbors
net arp
```

Muestra vecinos IP/MAC y añade:

- tipo de MAC;
- fabricante cuando puede identificarse por OUI.

---

## `net ports`

```bash
net ports HOST
net ports HOST 22,80,443
```

Sin lista explícita prueba:

```text
22, 80, 443, 445, 3389
```

Cada conexión tiene un timeout corto y se presenta como:

```text
open
closed/filtered
```

Ejemplo:

```bash
net ports 10.6.21.10 22,80,443,445
```

---

# 4.1 Descubrimiento — `net scan`

```bash
net scan
net scan RED
net scan RED --names
net scan RED --unknown
net scan RED --authorized
net scan RED --known
net scan RED --json
net scan RED --csv
```

## Red automática

```bash
net scan
```

Si no entregas una red, SST intenta determinar la IPv4 local y usa la red correspondiente.

## Red explícita

```bash
net scan 10.11.24.0/24
```

El escaneo está limitado a `/20` o redes más pequeñas para evitar barridos excesivos.

## Filtros

```bash
net scan 10.11.24.0/24 --unknown
```

Muestra sólo equipos no inventariados.

```bash
net scan 10.11.24.0/24 --known
```

Muestra equipos ya conocidos.

```bash
net scan 10.11.24.0/24 --authorized
```

Usa el filtro de autorización disponible en el inventario/descubrimiento.

## Nombres

```bash
net scan 10.11.24.0/24 --names
```

Solicita resolución de nombres.

## Salida estructurada

```bash
net scan 10.11.24.0/24 --json
net scan 10.11.24.0/24 --csv
```

---

# 4.2 Monitor de presencia — `net monitor`

```bash
net monitor RED
net monitor RED --unknown
net monitor RED --message TEXTO
net monitor RED --say TEXTO
```

Ejemplo:

```bash
net monitor 10.11.24.0/24
```

Mantiene una TUI con barridos periódicos y registra:

- equipos que aparecen;
- equipos que desaparecen;
- cambios de IP;
- historial de presencia;
- datos de inventario disponibles.

Controles:

```text
Enter  abre "Mensaje>"
Esc    cancela el mensaje o sale, según el contexto
q      sale cuando no estás editando un mensaje
```

`--message` y `--say` permiten iniciar el monitor publicando un mensaje SST en la LAN.

---

# 4.3 Presencia — `net presence`

```bash
net presence
net presence --json
net presence --csv
```

Consulta el historial persistente de dispositivos observados.

Incluye, según disponibilidad:

- MAC;
- IP;
- hostname;
- tipo de MAC;
- fabricante;
- método de descubrimiento;
- primera observación;
- última observación.

---

# 4.4 Identificación — `net identify`

```bash
net identify IP
net identify MAC
net identify NOMBRE
net identify OBJETIVO --json
```

Acepta:

- una IPv4;
- una MAC;
- el nombre de un dispositivo del inventario.

Correlaciona información actual e histórica y puede mostrar:

- IP;
- hostname;
- MAC;
- tipo global/local;
- fabricante;
- registro IEEE;
- nombre inventariado;
- si está registrado;
- método de descubrimiento;
- tiempo de respuesta;
- última observación.

Ejemplo:

```bash
net identify 10.11.24.15
```

---

# 4.5 Tráfico — `net traffic`

```bash
net traffic
net traffic --watch
net traffic --top N
net traffic --pid PID
net traffic --process NOMBRE
net traffic --background
net traffic --high-usage
net traffic --unsigned
net traffic --connections
net traffic --json
net traffic --csv
```

## Vista normal

```bash
net traffic
```

Toma una muestra ETW aproximada de 1,2 segundos cuando ETW está disponible y combina:

- upload/s;
- download/s;
- CPU;
- RAM;
- conexiones;
- foreground/background;
- firma Authenticode;
- PID/PPID.

## Monitor

```bash
net traffic --watch
```

Actualiza la vista aproximadamente cada segundo.

Salir:

```text
q
Esc
```

## Top

```bash
net traffic --top 20
```

Limita el resultado.

## Filtrar PID

```bash
net traffic --pid 8788
```

## Filtrar nombre

```bash
net traffic --process msedgewebview2
```

El filtro de nombre admite coincidencia parcial.

## Sólo background

```bash
net traffic --background
```

Excluye el proceso que SST detecta como foreground.

## Alto consumo

```bash
net traffic --high-usage
```

Filtra procesos con actividad de red significativa o CPU elevada según los umbrales implementados.

## Firma

```bash
net traffic --unsigned
```

Oculta los que tienen firma Authenticode válida, dejando unsigned/unknown/otros estados.

## Ver destinos y puertos

```bash
net traffic --connections
```

Muestra las conexiones con proceso propietario en vez de la tabla de velocidades.

## JSON/CSV

```bash
net traffic --json
net traffic --csv
```

---

# 4.6 Consumo de LAN — `net usage`

```bash
net usage
net usage --watch
net usage --top N
net usage --device IP
net usage --mac MAC
net usage --json
net usage --csv
```

Esta herramienta necesita un proveedor activo que entregue contadores por cliente.

Si no hay uno:

```text
net usage: no hay proveedor activo
```

## Monitor

```bash
net usage --watch
```

Actualiza aproximadamente cada dos segundos.

## Filtros

```bash
net usage --device 10.11.24.15
net usage --mac AA:BB:CC:DD:EE:FF
net usage --top 10
```

---

# 4.7 Proveedores — `net provider`

```bash
net provider list
net provider list --json

net provider add NOMBRE --type TIPO --host HOST
net provider add NOMBRE --type TIPO --host HOST --user-env VAR
net provider add NOMBRE --type TIPO --host HOST --secret-env VAR
net provider add NOMBRE --type TIPO --host HOST --community-env VAR

net provider use NOMBRE
net provider current
net provider remove NOMBRE

net provider capabilities
net provider capabilities NOMBRE
net provider path
```

Tipos admitidos:

```text
openwrt
opnsense
pfsense
unifi
snmp
generic
```

Las credenciales se referencian mediante variables de entorno; la intención es evitar guardarlas directamente como texto plano en el perfil.

`generic` y `openwrt` consumen actualmente JSON normalizado. Los perfiles `opnsense`, `pfsense` y `unifi` requieren el endpoint/API concreto de cada instalación. El SNMP estándar no define por sí solo contadores de consumo por cliente.

---

# 5. Inventario de equipos — `device`

## `device list`

```bash
device list
device list --json
device list --csv
```

Muestra el inventario local por MAC, incluyendo nombre, tipo de MAC, fabricante y notas.

---

## `device show`

```bash
device show MAC
device show NOMBRE
```

Muestra:

- nombre;
- MAC;
- tipo de MAC;
- fabricante;
- registro IEEE;
- última IP;
- último hostname;
- última observación;
- notas.

---

## `device add`

```bash
device add MAC NOMBRE
device add IP NOMBRE
device add MAC NOMBRE --note TEXTO
```

### Por MAC

```bash
device add AA:BB:CC:DD:EE:FF Impresora-Biblioteca
```

### Por IP

```bash
device add 10.11.24.50 Impresora-Biblioteca
```

SST primero intenta resolver la MAC del vecino.

Si no puede resolverla, puede ser necesario ejecutar `net scan` o comprobar que el equipo esté en el mismo segmento.

### Nota

```bash
device add AA:BB:CC:DD:EE:FF Notebook-Juan --note "Sala 3"
```

Si la MAC ya existe, actualiza su información.

---

## `device remove`

```bash
device remove MAC
```

Elimina el dispositivo del inventario.

---

## `device unknown`

```bash
device unknown
device unknown --json
device unknown --csv
```

Muestra elementos presentes en el historial de presencia cuya MAC no está registrada en el inventario.

---

## `device path`

```bash
device path
```

Muestra dónde se guarda el inventario.

---

# 6. Dominio — `domain`

```bash
domain status
domain status EQUIPO
domain status EQUIPO --verify
domain status EQUIPO --cim
domain status EQUIPO --json
```

## Equipo local

```bash
domain status
```

Muestra:

- hostname;
- dominio;
- si está unido;
- fuente;
- nivel de confianza;
- logon server cuando está disponible.

## Equipo remoto

```bash
domain status PC-123
```

## Verificación adicional

```bash
domain status PC-123 --verify
```

`--cim` activa la misma intención de verificación remota disponible en el servicio.

## JSON

```bash
domain status PC-123 --json
```

---

# 7. Switches SNMP — `switch`

## `switch list`

```bash
switch list
switch list --json
```

Muestra switches configurados.

---

## `switch add`

```bash
switch add NOMBRE --host HOST --community-env VARIABLE
```

Ejemplo:

```bash
switch add SW-PISO2 --host 10.0.0.10 --community-env SST_SNMP_COMMUNITY
```

Opcional:

```bash
switch add SW-PISO2 --host 10.0.0.10 --community-env SST_SNMP_COMMUNITY --description "Piso 2"
```

La comunidad SNMP se toma desde una variable de entorno.

---

## `switch show`

```bash
switch show NOMBRE
```

Muestra host, variable de comunidad y descripción.

---

## `switch remove`

```bash
switch remove NOMBRE
```

Elimina el perfil del switch.

---

## `switch locate`

```bash
switch locate MAC
switch locate NOMBRE
switch locate MAC --switch NOMBRE
switch locate MAC --vlan N
switch locate MAC --json
```

Busca la MAC en los switches SNMP configurados.

Puede resolver:

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
 ↓
VLAN/PVID
 ↓
velocidad/estado
```

También acepta el nombre de un equipo inventariado en lugar de su MAC.

---

## `switch capabilities`

```bash
switch capabilities
```

Muestra las capacidades del localizador SNMP implementado.

---

## `switch path`

```bash
switch path
```

Muestra la ruta del archivo de perfiles de switches.

---

# 8. Wake-on-LAN — `wol`

```bash
wol MAC
wol NOMBRE
wol MAC BROADCAST
```

Envía un Magic Packet.

Ejemplos:

```bash
wol AA:BB:CC:DD:EE:FF
wol PC-BIBLIOTECA
wol AA:BB:CC:DD:EE:FF 10.11.24.255:9
```

Si se entrega un nombre, debe existir en el inventario de `device`.

Por defecto usa:

```text
255.255.255.255:9
```

---

# 9. Diagnósticos compuestos — `diag`

## `diag network`

```bash
diag network
```

Ejecuta el diagnóstico compuesto de red disponible en `NetworkService`.

---

## `diag dns`

```bash
diag dns
diag dns HOST
```

Sin HOST usa el objetivo por defecto definido en la implementación. Con HOST invoca `net dns`.

---

## `diag hardware`

```bash
diag hardware
```

Concatena:

```text
sys info
sys memory
sys disks
```

---

## `diag storage`

```bash
diag storage
```

Equivale a la vista de discos.

---

## `diag traffic`

```bash
diag traffic
```

Lanza la vista de tráfico de red.

---

## `diag domain`

```bash
diag domain
```

Consulta el estado de dominio local.

---

# 10. Event Log — `eventlog`

`eventlog` es la interfaz Nwash completa sobre Windows Event Log. A diferencia de `sys events`, incluye operaciones modificables.

## Listar logs

```bash
eventlog list
```

## Listar publishers

```bash
eventlog publishers
```

## Información de un log

```bash
eventlog info System
```

## Leer eventos

```bash
eventlog read
eventlog read System
eventlog read System --count 50
eventlog read System --query XPATH
eventlog read System --xml
```

Por defecto:

- log `System`;
- 20 eventos;
- salida de texto.

## Exportar

```bash
eventlog export System system.evtx
eventlog export System system.evtx --query XPATH
eventlog export System system.evtx --overwrite
```

## Limpiar

```bash
eventlog clear LOG
eventlog clear LOG --backup ARCHIVO
```

Ejemplo:

```bash
sudo eventlog clear Application --backup application.evtx
```

Limpiar logs puede requerir elevación.

---

# 11. Servicios — `service`

Interfaz modificable de servicios Windows.

## Listar

```bash
service list
service list --running
service list --stopped
```

## Estado

```bash
service status Spooler
```

## Control

```bash
service start Spooler
service stop Spooler
service pause NOMBRE
service resume NOMBRE
service restart NOMBRE
```

`restart` espera a que el servicio alcance `STOPPED` antes de volver a iniciarlo.

Las operaciones de control pueden requerir:

```bash
sudo service ...
```

---

# 12. Registro — `registry`

Interfaz modificable del Registro.

## Leer

```bash
registry get CLAVE
registry get CLAVE --value NOMBRE
registry get CLAVE --default
registry get CLAVE --recursive
```

## Crear/modificar valor

```bash
registry set CLAVE NOMBRE DATO
registry set CLAVE NOMBRE DATO --type TIPO
```

Ejemplo:

```bash
registry set "HKCU\Software\MiApp" Estado activo --type REG_SZ
```

## Valor predeterminado

```bash
registry set-default CLAVE DATO
```

## Eliminar

```bash
registry delete CLAVE --value NOMBRE
registry delete CLAVE --default
registry delete CLAVE --key
```

Para evitar eliminaciones ambiguas, debes indicar expresamente qué quieres borrar.

## Exportar/importar

```bash
registry export CLAVE ARCHIVO
registry export CLAVE ARCHIVO --overwrite
registry import ARCHIVO
```

Tipos admitidos por la interfaz:

```text
REG_SZ
REG_EXPAND_SZ
REG_DWORD
REG_QWORD
REG_MULTI_SZ
REG_BINARY
```

Las mutaciones pueden requerir `sudo`.

---

# 13. Procesos Nwash — `process`

## Listar

```bash
process list
```

Usa `tasklist`.

## Información

```bash
process info PID
```

Solicita vista detallada mediante `tasklist /V`.

## Árbol

```bash
process tree PID
```

Construye el árbol del PID indicado.

## Terminar

```bash
process kill PID
process kill PID --tree
process kill PID --force
```

Esta herramienta usa `taskkill`.

- `--tree`: añade `/T`;
- `--force`: añade `/F`.

Para la implementación nativa Win32 de SST usa `sys kill`.

---

# 14. ACL NTFS — `acl`

## Ver

```bash
acl show RUTA
```

## Conceder

```bash
acl grant RUTA USUARIO PERMISO
```

## Denegar

```bash
acl deny RUTA USUARIO PERMISO
```

## Revocar

```bash
acl revoke RUTA USUARIO
```

## Herencia

```bash
acl inherit RUTA on
acl inherit RUTA off
```

## Reset

```bash
acl reset RUTA
```

Permisos habituales de `icacls`:

```text
F   Full control
M   Modify
RX  Read & execute
R   Read
W   Write
```

Las operaciones modificables pueden requerir elevación.

---

# 15. Plug and Play — `pnp`

## Listar

```bash
pnp list
pnp list --connected
pnp list --disconnected
pnp list --problem
```

Usa `pnputil`.

## Información

```bash
pnp info INSTANCE_ID
```

Muestra propiedades del dispositivo.

## Control

```bash
pnp enable INSTANCE_ID
pnp disable INSTANCE_ID
pnp restart INSTANCE_ID
```

## Reescaneo

```bash
pnp scan
```

Solicita a Windows un nuevo escaneo de dispositivos.

Las operaciones de control/scan pueden requerir `sudo`.

---

# 16. Tareas programadas — `task`

## Listar

```bash
task list
task list --verbose
task list --csv
```

## Mostrar

```bash
task show NOMBRE
task show NOMBRE --xml
```

## Ejecutar/detener

```bash
task run NOMBRE
task end NOMBRE
```

## Habilitar/deshabilitar

```bash
task enable NOMBRE
task disable NOMBRE
```

## Eliminar

```bash
task delete NOMBRE
```

La eliminación usa la opción forzada de `schtasks`.

Las mutaciones pueden requerir `sudo`.

---

# 17. Sesiones Windows/RDP — `session`

## Listar sesiones

```bash
session list
```

## Listar usuarios conectados

```bash
session users
```

## Cerrar una sesión

```bash
session logoff ID
```

## Enviar un mensaje

```bash
session message ID TEXTO
```

Ejemplo:

```bash
session message 2 "El equipo se reiniciará en 10 minutos"
```

`logoff` y `message` pueden requerir elevación según la sesión objetivo.

---

# 18. SMB — `share`

## Compartidos

```bash
share list
```

## Sesiones SMB

```bash
share sessions
```

## Archivos abiertos por SMB

```bash
share files
```

## Crear recurso

```bash
share add NOMBRE RUTA
```

Ejemplo:

```bash
share add Publico C:\Publico
```

## Eliminar recurso

```bash
share remove NOMBRE
```

Las mutaciones pueden requerir `sudo`.

---

# 19. Firewall — `firewall`

## Estado

```bash
firewall status
```

Muestra todos los perfiles de Windows Defender Firewall.

## Reglas

```bash
firewall rules
```

## Regla concreta

```bash
firewall rule NOMBRE
```

## Activar

```bash
firewall enable domain
firewall enable private
firewall enable public
firewall enable all
```

## Desactivar

```bash
firewall disable domain
firewall disable private
firewall disable public
firewall disable all
```

Las operaciones que cambian el estado pueden requerir `sudo`.

---

# 20. Energía — `power`

## Apagar

```bash
power shutdown
power shutdown --force
```

## Reiniciar

```bash
power restart
power restart --force
```

## Cerrar sesión

```bash
power logoff
power logoff --force
```

## Hibernar

```bash
power hibernate
```

`--force` solicita cierre forzado de aplicaciones.

Apagar/reiniciar puede requerir elevación según la política del equipo.

---

# 21. Elevación — `sudo`

Alias:

```bash
runas
```

> `runas` es alias interno de SST. No invoca `runas.exe`.

## Ver estado

```bash
sudo --status
```

Muestra:

- nivel SST;
- siguiente nivel;
- identidad;
- estado de elevación;
- nivel de integridad;
- privilegios `Se*` y si están habilitados.

## Elevar la sesión

```bash
sudo
```

Sin argumentos sube la sesión al siguiente nivel de la escalera implementada:

```text
USER
  ↓
ADMIN_FILTERED
  ↓
ADMINISTRATOR
  ↓
LOCAL_SYSTEM
  ↓
TRUSTEDINSTALLER
```

## Ejecutar como Administrador

```bash
sudo COMANDO [ARGS...]
```

Ejemplo:

```bash
sudo service restart Spooler
```

## Ejecutar como SYSTEM

```bash
sudo --system COMANDO [ARGS...]
```

## Ejecutar como TrustedInstaller

```bash
sudo --trustedinstaller COMANDO [ARGS...]
```

Las elevaciones iniciales usan UAC cuando corresponde.

---

# 22. Editor — Helix SST

Comandos equivalentes:

```bash
helix
hx
helix-sst
```

Abrir archivo:

```bash
hx archivo.txt
```

Ayuda:

```bash
helix --help
helix --guide
```

Versión:

```bash
helix --version
```

Créditos/upstream:

```bash
helix --credits
```

El editor está basado en Helix y SST mantiene su integración propia.

---

# 23. Configuración SST

## `config path`

```bash
config path
```

Muestra la ruta del archivo de configuración cargado.

## `config edit`

```bash
config edit
```

Abre la configuración en Helix SST.

## `config reload`

```bash
config reload
```

Vuelve a leer el archivo indicado por `SST_CONFIG` dentro de la shell actual.

---

## Fondos — `config bg`

### Listar

```bash
config bg
```

Muestra:

- carpeta `bg`;
- modo actual;
- imágenes disponibles;
- imagen seleccionada.

### Seleccionar por nombre

```bash
config bg background01.png
```

### Seleccionar por número

```bash
config bg 2
```

### Carrusel

```bash
config bg carrousel
config bg carrousel 5
```

Activa rotación de fondos. El intervalo debe estar entre 1 y 60 minutos.

También acepta:

```bash
config bg carousel
```

### Siguiente

```bash
config bg next
```

Selecciona la siguiente imagen y deja el modo fijo.

### Desactivar

```bash
config bg off
```

---

## Comandos internos relacionados

```text
sst-config
sst-path
```

`sst-config` es el builtin interno detrás de parte de `config`.

`sst-path RUTA` traduce rutas estilo `/c/...` a una ruta de Windows utilizable por SST.

---

# 24. Tour / ejemplos — `tour`

```bash
tour
```

Abre un menú interactivo que busca scripts en `examples/`.

Controles:

```text
↑ / ↓        mover selección
PgUp/PgDn    saltar
Home/End     inicio/final
Enter        elegir
q / Esc      salir
```

Ejemplos previstos en el menú actual:

```text
01 · Lenguaje Nwash
02 · Operador Windows
03 · Descubrimiento de red
04 · Jobs y coprocesos
05 · Pipelines y texto
06 · Security triage
07 · Operator console
08 · Config y fondos
09 · Bash avanzado
10 · Full showcase
```

Al elegir un ejemplo, SST devuelve la ruta del script para que la shell lo ejecute.

---

# 25. Herramientas Unix integradas

Estas herramientas ofrecen una capa Unix portátil. Algunas coinciden con builtins Bash (`pwd`, `echo`, `printf`, `true`, `false`, `type`); en esos casos la resolución de la shell puede usar primero el builtin Bash.

## `pwd`

```bash
pwd
```

Muestra el directorio actual con formato de ruta estilo Unix.

---

## `echo`

```bash
echo ARG...
```

Imprime los argumentos separados por espacios.

En la shell normal existe además el builtin Bash `echo`, que soporta opciones compatibles con Bash.

---

## `env`

```bash
env
```

Lista las variables de entorno ordenadas por nombre.

---

## `clear`

```bash
clear
```

Limpia la terminal mediante secuencias ANSI.

---

## `ls`

```bash
ls
ls RUTA
ls -a
ls -l
ls -la
```

- `-a`: incluye nombres que empiezan por `.`.
- `-l`: añade tipo, estado readonly, tamaño y nombre.

Ejemplo:

```bash
ls -la .
```

---

## `cat`

```bash
cat ARCHIVO
cat ARCHIVO1 ARCHIVO2
comando | cat
```

Concatena archivos o, sin argumentos, usa stdin.

---

## `head`

```bash
head ARCHIVO
head -n 20 ARCHIVO
comando | head -n 20
```

Por defecto muestra 10 líneas.

---

## `tail`

```bash
tail ARCHIVO
tail -n 20 ARCHIVO
comando | tail -n 20
```

Por defecto muestra las últimas 10 líneas.

---

## `grep`

```bash
grep PATRON [ARCHIVO]
grep -i PATRON [ARCHIVO]
grep -n PATRON [ARCHIVO]
grep -in PATRON [ARCHIVO]
```

La implementación actual busca texto literal por línea.

- `-i`: ignora mayúsculas/minúsculas.
- `-n`: muestra número de línea.

Devuelve status 1 cuando no encuentra coincidencias.

---

## `wc`

```bash
wc [ARCHIVO]
comando | wc
```

Muestra:

```text
líneas palabras bytes
```

---

## `sort`

```bash
sort [ARCHIVO]
sort -r [ARCHIVO]
```

Ordena líneas lexicográficamente.

`-r` invierte el orden.

---

## `uniq`

```bash
uniq [ARCHIVO]
comando | uniq
```

Elimina líneas **adyacentes** repetidas.

Uso típico:

```bash
cat archivo.txt | sort | uniq
```

---

## `cut`

```bash
cut -d DELIMITADOR -f CAMPO [ARCHIVO]
```

Ejemplo:

```bash
cut -d "," -f 2 datos.csv
```

El campo empieza en 1.

---

## `xargs`

El nombre `xargs` está registrado en la lista de comandos Unix del proyecto, pero el `UnixService` actual no tiene una rama de ejecución para `xargs`.

Por tanto, en el estado actual debe considerarse **registrado pero no implementado funcionalmente** hasta conectar su handler.

---

## `tee`

```bash
comando | tee ARCHIVO
comando | tee -a ARCHIVO
```

- sin `-a`: sobrescribe;
- `-a`: agrega.

También deja el mismo texto en stdout.

---

## `less` / `more`

```bash
less ARCHIVO
more ARCHIVO
comando | less
```

Paginador interactivo.

Controles:

```text
j / ↓        bajar una línea
k / ↑        subir una línea
PgDn / Space bajar una página
PgUp         subir una página
g / Home     inicio
G / End      final
q / Esc      salir
```

---

## `sed`

La implementación actual soporta sustitución:

```bash
sed 's/antiguo/nuevo/' ARCHIVO
sed 's/antiguo/nuevo/g' ARCHIVO
```

También puede recibir stdin.

No es un `sed` GNU completo.

---

## `awk`

Soporta principalmente expresiones de tipo `print`.

```bash
awk '{print $1}' ARCHIVO
awk -F "," '{print $1,$3}' datos.csv
```

Campos disponibles:

- `$0`: línea completa;
- `$1`, `$2`, etc.;
- `NR`: número de línea.

No es una implementación completa de awk.

---

## `diff`

```bash
diff ARCHIVO1 ARCHIVO2
```

Muestra diferencias de líneas con encabezados `---` y `+++`.

- status 0: archivos iguales;
- status 1: diferencias.

---

## `sha256sum`

```bash
sha256sum ARCHIVO
comando | sha256sum
```

Calcula SHA-256.

---

## `base64`

```bash
base64 ARCHIVO
comando | base64

base64 -d ARCHIVO
base64 --decode ARCHIVO
```

Codifica o decodifica Base64.

---

## `find`

```bash
find
find RUTA
find RUTA -name PATRON
```

Recorre recursivamente desde la ruta indicada.

La implementación actual soporta principalmente `-name`.

---

## `printf`

```bash
printf FORMATO [ARG...]
```

La capa Unix soporta:

```text
%s
%d
%%
\n
\t
\r
\\
```

En la shell normal existe además el builtin Bash `printf`, que tiene mayor compatibilidad y soporta `-v`.

---

## `basename`

```bash
basename RUTA
```

Devuelve el último componente.

---

## `dirname`

```bash
dirname RUTA
```

Devuelve la parte de directorio.

---

## `realpath`

```bash
realpath RUTA
```

Canonicaliza la ruta. La ruta debe poder resolverse.

---

## `date`

```bash
date
date "+%Y-%m-%d %H:%M:%S"
```

Sin formato usa uno similar a:

```text
%a %b %e %H:%M:%S %Y
```

---

## `sleep`

```bash
sleep DURACION
```

Espera el intervalo solicitado según los sufijos admitidos por el parser de duración de SST.

---

## `true`

```bash
true
```

No imprime nada y devuelve status 0.

---

## `false`

```bash
false
```

No imprime nada y devuelve status 1.

---

## `touch`

```bash
touch ARCHIVO [ARCHIVO...]
```

Crea el archivo si no existe sin truncarlo.

---

## `mkdir`

```bash
mkdir DIRECTORIO
mkdir -p RUTA/ANIDADA
```

`-p` crea padres faltantes.

---

## `rm`

```bash
rm ARCHIVO
rm -r DIRECTORIO
rm -f ARCHIVO
rm -rf DIRECTORIO
```

- `-r`: recursivo;
- `-f`: ignora archivos inexistentes.

---

## `cp`

```bash
cp ORIGEN DESTINO
```

La implementación actual copia un archivo con exactamente dos operandos. No implementa copia recursiva de directorios.

---

## `mv`

```bash
mv ORIGEN DESTINO
```

Renombra o mueve usando la operación del sistema de archivos.

---

## `tar`

Modos implementados:

```bash
tar -cf ARCHIVO.tar ARCHIVOS...
tar -czf ARCHIVO.tar.gz ARCHIVOS...
tar -tf ARCHIVO.tar
tar -xf ARCHIVO.tar
tar -xzf ARCHIVO.tar.gz
```

También admite:

```bash
tar ... -C DIRECTORIO
```

Flags disponibles:

```text
c  crear
x  extraer
t  listar
z  gzip
v  verbose
f  archivo
```

La implementación requiere `-f ARCHIVO`.

---

## `gzip`

```bash
gzip ARCHIVO
gzip -k ARCHIVO
gzip -d ARCHIVO.gz
```

- comprime a `ARCHIVO.gz`;
- por defecto elimina el original;
- `-k`/`--keep` conserva el original;
- `-d`/`--decompress` descomprime.

Actualmente `-c`/`--stdout` no está disponible porque el pipeline SST para esta ruta es textual, no binario.

---

## `gunzip`

```bash
gunzip ARCHIVO.gz
gunzip -k ARCHIVO.gz
```

Es la forma directa de descompresión de `gzip`.

---

## `zip`

```bash
zip ARCHIVO.zip ARCHIVO1 ARCHIVO2
zip -r ARCHIVO.zip DIRECTORIO
```

Los directorios requieren `-r`.

---

## `unzip`

```bash
unzip ARCHIVO.zip
unzip -l ARCHIVO.zip
unzip ARCHIVO.zip -d DIRECTORIO
```

- `-l`: sólo lista;
- `-d`: destino;
- `-o`: se acepta en el parser actual.

La extracción valida que las entradas no escapen del directorio destino.

---

## `which`

```bash
which COMANDO
```

Indica si el nombre corresponde a un comando interno o muestra la ruta encontrada en `PATH`.

---

## `type`

`type` existe como nombre Unix registrado, pero en la shell normal SST lo resuelve como builtin Bash.

Uso recomendado:

```bash
type COMANDO
type -a COMANDO
```

Consulta la sección de builtins Bash.

---

# 26. Builtins Bash/Nwash

Estos comandos pertenecen al intérprete de shell y, por tanto, modifican o consultan el estado de la shell actual.

## `:`

```bash
:
```

No hace nada y devuelve status 0.

Se usa a menudo como no-op:

```bash
while true; do
    :
done
```

---

## `.` / `source`

```bash
source ARCHIVO [ARGS]
. ARCHIVO [ARGS]
```

Ejecuta el archivo dentro de la shell actual, por lo que sus variables, funciones y cambios de directorio pueden permanecer después.

---

## `test` / `[`

```bash
test EXPRESION
[ EXPRESION ]
```

Evalúa condiciones para scripts.

Ejemplo:

```bash
if [ -f archivo.txt ]; then
    echo existe
fi
```

---

## `alias`

```bash
alias
alias NOMBRE='COMANDO'
```

Sin argumentos lista alias. Con asignación crea o reemplaza uno.

---

## `unalias`

```bash
unalias NOMBRE
unalias -a
```

Elimina uno o todos los alias.

---

## `cd`

```bash
cd [DIR]
cd -L DIR
cd -P DIR
```

Cambia el directorio de la shell actual.

---

## `pwd`

```bash
pwd
pwd -L
pwd -P
```

Muestra el directorio actual.

---

## `echo`

```bash
echo [-neE] [ARG...]
```

Builtin Bash para imprimir texto.

---

## `printf`

```bash
printf [-v VAR] FORMATO [ARG...]
```

Imprime usando formato Bash.

Con `-v` guarda el resultado en una variable en vez de imprimirlo.

---

## `export`

```bash
export NOMBRE
export NOMBRE=VALOR
```

Marca variables para que sean heredadas por procesos hijos.

---

## `unset`

```bash
unset NOMBRE
unset -v NOMBRE
unset -f FUNCION
```

Elimina variables o funciones.

---

## `local`

```bash
local NOMBRE=VALOR
```

Declara variables locales dentro de una función.

---

## `declare` / `typeset`

```bash
declare [OPCIONES] NOMBRE[=VALOR]
typeset [OPCIONES] NOMBRE[=VALOR]
```

Declara variables y atributos, incluyendo arrays/nameref según las opciones implementadas.

---

## `readonly`

```bash
readonly NOMBRE
readonly NOMBRE=VALOR
```

Marca una variable o función como sólo lectura.

---

## `read`

```bash
read [OPCIONES] [NOMBRE...]
```

Lee una línea o registro hacia variables.

La implementación SST incluye soporte ampliado para opciones Bash de lectura; consulta:

```bash
help read
```

para el detalle reconocido por la build actual.

---

## `mapfile` / `readarray`

```bash
mapfile [OPCIONES] [ARRAY]
readarray [OPCIONES] [ARRAY]
```

Lee registros en un array indexado.

---

## `set`

```bash
set [OPCIONES]
set -o OPCION
set +o OPCION
set -- ARG...
```

Configura opciones de la shell y parámetros posicionales.

Entre las opciones modeladas por SST se encuentran conceptos Bash como:

```text
errexit
nounset
xtrace
noglob
monitor
pipefail
posix
```

---

## `shopt`

```bash
shopt
shopt -s OPCION
shopt -u OPCION
```

Configura opciones adicionales de Bash.

SST modela, entre otras:

```text
dotglob
globstar
nullglob
failglob
nocaseglob
extglob
expand_aliases
lastpipe
histappend
extdebug
```

---

## `break`

```bash
break [N]
```

Sale del bucle actual o de `N` niveles.

---

## `continue`

```bash
continue [N]
```

Continúa la siguiente iteración del bucle, opcionalmente subiendo niveles.

---

## `return`

```bash
return [N]
```

Retorna desde una función o archivo ejecutado con `source`.

---

## `shift`

```bash
shift [N]
```

Desplaza los parámetros posicionales `$1`, `$2`, etc.

---

## `exit`

```bash
exit [N]
```

Cierra la shell con el código indicado.

---

## `logout`

```bash
logout [N]
```

Equivalente de salida pensado para contexto de login shell.

---

## `trap`

```bash
trap [-lp] [[ARG] SEÑAL...]
```

Configura acciones ante señales y pseudo-señales de Bash.

---

## `eval`

```bash
eval ARG...
```

Concatena y evalúa los argumentos como código shell.

---

## `let`

```bash
let EXPRESION...
```

Evalúa expresiones aritméticas.

---

## `jobs`

```bash
jobs
jobs [OPCIONES] [JOB...]
```

Lista trabajos gestionados por job control.

---

## `wait`

```bash
wait [ID...]
wait -n
wait -p VAR
```

Espera procesos o jobs.

---

## `fg`

```bash
fg [JOB]
```

Trae un job al primer plano.

---

## `bg`

```bash
bg [JOB...]
```

Continúa jobs en segundo plano.

---

## `disown`

```bash
disown [JOB...]
```

Elimina jobs de la tabla de jobs, con opciones compatibles modeladas por SST.

---

## `command`

```bash
command [-pVv] COMANDO [ARGS...]
```

Ejecuta o describe un comando omitiendo la resolución de funciones de shell.

---

## `builtin`

```bash
builtin BUILTIN [ARGS...]
```

Fuerza la ejecución de un builtin.

---

## `type`

```bash
type NOMBRE
type -a NOMBRE
```

Describe cómo resolvería la shell un nombre: builtin, alias, función, ejecutable, etc.

---

## `hash`

```bash
hash [OPCIONES] [NOMBRE...]
```

Gestiona la caché de localización de comandos.

---

## `getopts`

```bash
getopts OPTSTRING NOMBRE [ARGS]
```

Parser de opciones para scripts Bash/Nwash.

---

## `exec`

```bash
exec [OPCIONES] [COMANDO [ARGS]]
```

Ejecuta/reemplaza el proceso de shell según la forma usada.

---

## `history`

```bash
history
history [OPCIONES] [N]
```

Muestra o modifica el historial.

SST también implementa expansión de historial estilo Bash, incluyendo formas como:

```text
!!
!-N
!N
!?texto?
!prefijo
```

y varios modificadores de palabras/rutas.

---

## `fc`

```bash
fc [-e EDITOR] [-lnr] [PRIMERO] [ULTIMO]
```

Lista, edita o reejecuta entradas del historial.

---

## `bind`

```bash
bind [OPCIONES] [SECUENCIA:FUNCION]
```

Configura parte de la edición de línea compatible con Bash.

No debe asumirse que sea GNU Readline completo.

---

## `enable`

```bash
enable [-a] [-dnps] [NOMBRE...]
```

Activa o desactiva builtins modelados por SST.

Las capacidades dinámicas de carga de builtins nativos no deben asumirse equivalentes a Bash GNU completo.

---

## `complete`

```bash
complete [OPCIONES] [NOMBRE...]
```

Define completado programable.

---

## `compgen`

```bash
compgen [OPCIONES] [PALABRA]
```

Genera candidatos de completion.

---

## `compopt`

```bash
compopt [-o OPCION] [+o OPCION] [NOMBRE...]
```

Modifica opciones del completion.

---

## `dirs`

```bash
dirs [-clpv] [+N|-N]
```

Muestra la pila de directorios.

---

## `pushd`

```bash
pushd [-n] [DIR|+N|-N]
```

Añade o rota la pila de directorios.

---

## `popd`

```bash
popd [-n] [+N|-N]
```

Quita una entrada de la pila.

---

## `umask`

```bash
umask
umask -S
umask MASCARA
```

Muestra o establece la máscara de creación modelada por SST.

---

## `ulimit`

```bash
ulimit [OPCIONES] [LIMITE]
```

Consulta o establece límites que SST pueda representar en Windows.

No debe asumirse paridad completa con los límites de recursos de un kernel Unix.

---

## `times`

```bash
times
```

Muestra tiempos de CPU de shell y procesos hijos.

---

## `caller`

```bash
caller [N]
```

Muestra un frame de la pila de llamadas de funciones/scripts.

---

## `help`

```bash
help
help COMANDO
help -s COMANDO
help -d COMANDO
```

Sin argumentos lista los builtins compatibles.

- `-s`: uso corto;
- `-d`: sólo descripción.

Ejemplo:

```bash
help read
```

---

## `kill` — builtin Bash

```bash
kill [-s SEÑAL | -n SEÑAL | -SEÑAL] PID|%JOB...
kill -l
```

A diferencia de `sys kill`, esta forma trabaja con semántica Bash de señales y jobs.

Para terminar árboles completos por PID/nombre en Windows, usa:

```bash
sys kill ... --tree
```

---

## `suspend`

```bash
suspend [-f]
```

Intenta suspender una shell interactiva cuando el host lo permite.

---

## `config`

```bash
config path
config edit
config reload
config bg ...
```

Está integrado como builtin de shell porque `reload` necesita modificar el contexto actual.

Consulta la sección **Configuración SST**.

---

## `reload`

```bash
reload
```

Atajo/builtin relacionado con la recarga de configuración del entorno SST según la implementación de la shell.

---

## `tour`

```bash
tour
```

Abre el selector de ejemplos explicado en la sección correspondiente.

---

## `true` / `false`

```bash
true
false
```

Estados de salida:

```text
true  -> 0
false -> 1
```

---

# 27. Capacidades del lenguaje Nwash/Bash implementadas

Además de los comandos, el intérprete incluye soporte para:

```text
pipelines
redirecciones
background jobs
job control
coprocesos
process substitution
variables y variables especiales
arrays indexados
arrays asociativos
nameref
funciones
if / elif / else
for
while
until
case
select
aritmética
[[ ... ]]
traps
globbing
extglob
expansión de historial
completion programable
```

Para comprobar la sintaxis de un builtin concreto:

```bash
help NOMBRE
```

---

# 28. Resumen rápido de herramientas principales

```text
sys       sistema Windows
triage    análisis/correlación de seguridad
intel     inteligencia de seguridad
net       red
device    inventario de equipos
domain    dominio
switch    switching/SNMP
wol       Wake-on-LAN
diag      diagnósticos compuestos
eventlog  eventos de Windows
service   servicios
registry  Registro de Windows
process   procesos mediante herramientas Windows
acl       permisos NTFS
pnp       dispositivos Plug and Play
task      tareas programadas
session   sesiones/RDP
share     SMB
firewall  Windows Defender Firewall
power     energía/sesión
sudo      elevación de privilegios
helix     editor
config    configuración SST
tour      ejemplos interactivos
```

---

# 29. Secuencias de trabajo útiles

## Investigar un proceso marcado por SST

```bash
triage --scan
triage PID
triage PID --deep
```

o por partes:

```bash
sys why PID
sys inspect PID --deep
sys diff PID
net traffic --pid PID
net traffic --connections --pid PID
```

## Investigar presión de memoria

```bash
sys memory
triage --memory
top
```

Dentro de `top`:

```text
m    ordenar por memoria
```

## Revisar un equipo antes de cambiarle nombre/IP

```bash
sys hostname
sys whoami
net interfaces
domain status
```

## Descubrir equipos de una LAN

```bash
net scan 10.11.24.0/24 --names
device unknown
net presence
```

## Encontrar dónde está conectado un equipo

```bash
net identify 10.11.24.15
switch locate AA:BB:CC:DD:EE:FF
```

## Revisar impresora instalada y su IP

```bash
sys printers
sys printer NOMBRE
sys printer NOMBRE --ip
```

## Revisar inicio automático y persistencia

```bash
sys startup
sys persistence
sys tasks --verbose
sys services --impact
```

## Revisar NAC desde herramientas existentes

SST todavía debe consolidar esta lógica en un comando específico `net nac`, pero el procedimiento manual documentado para el proyecto combina:

```bash
netsh lan show interfaces
netsh lan show profiles
ipconfig /all
ping <gateway>
ping <recurso_interno>
nslookup <host_interno>
```

La interpretación debe correlacionar:

- estado 802.1X;
- DHCP/IP;
- posible APIPA `169.254.x.x`;
- posible VLAN/subred de cuarentena;
- gateway;
- acceso a recursos internos;
- DNS.

La confirmación absoluta de la decisión del NAC puede requerir revisar switch/RADIUS/NAC.
