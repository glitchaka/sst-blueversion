# SST Security Triage Plan

## Propósito

SST debe incorporar capacidades de **triage local, investigación y respuesta manual ante procesos potencialmente peligrosos**, sin convertirse en un antivirus autónomo ni en una herramienta que mate procesos automáticamente.

El objetivo es que SST pueda responder, con evidencia verificable, tres preguntas:

1. **¿Qué está ocurriendo realmente en este equipo?**
2. **¿Qué proceso merece atención y por qué?**
3. **¿Qué puedo contener o terminar sin destruir evidencia ni romper Windows?**

El principio operativo es:

```text
OBSERVAR -> CORRELACIONAR -> ADVERTIR -> EXPLICAR -> PROFUNDIZAR -> ACTUAR
```

No:

```text
DETECTAR -> MATAR
```

SST debe señalar anomalías y permitir investigar. La decisión final de contener, suspender, matar o poner en cuarentena sigue siendo del operador.

---

## Contexto de amenaza

Una motivación concreta para este módulo es responder mejor a incidentes como cadenas de ingeniería social tipo **ClickFix**, donde una página comprometida o un CAPTCHA falso induce al usuario a copiar y pegar un comando en PowerShell.

Una cadena relevante puede verse así:

```text
Navegador
  -> PowerShell / cmd
     -> descarga o crea ejecutable
        -> ejecutable en ruta escribible por el usuario
           -> persistencia
           -> conexión saliente
```

SST no debe marcar como malware un proceso por una sola señal. Debe detectar y explicar **combinaciones de comportamiento**.

---

# 1. Arquitectura

La arquitectura debe separar estrictamente:

- observación;
- análisis;
- respuesta privilegiada.

```text
+------------------------------------------------------------+
|                        SST GUI / SHELL                     |
|                                                            |
| ps · inspect · suspicious · net · persistence · timeline  |
+-----------------------------+------------------------------+
                              |
                     IPC local autenticado
                              |
                +-------------v--------------+
                |      SST SYSTEM BROKER     |
                |     Servicio LocalSystem   |
                |                            |
                | process.inspect            |
                | process.suspend            |
                | process.resume             |
                | process.terminate          |
                | process.dump               |
                | network.block              |
                | persistence.disable        |
                +-------------+--------------+
                              |
       +----------------------+----------------------+
       |                      |                      |
 Windows APIs / ETW      Registro / EventLog      Filesystem
```

La GUI no debe ejecutarse permanentemente como SYSTEM.

SST normal observa todo lo posible con el token del usuario. Sólo las operaciones que realmente lo requieran deben pasar por el broker privilegiado.

---

# 2. Privilegios necesarios

Para el módulo de procesos, la escalera útil es:

```text
Usuario
  -> Administrador elevado
     -> LOCAL SYSTEM
```

## Administrador

Se obtiene mediante UAC y un token elevado real.

Debe permitir:

- inspección ampliada;
- acceso a más procesos;
- activación de privilegios `Se*` presentes en el token;
- operaciones administrativas normales.

## LOCAL SYSTEM

Debe ser el nivel de respaldo para:

- procesos de otras cuentas;
- servicios;
- ACL de proceso más restrictivas;
- inspección y terminación privilegiada;
- uso de `SeDebugPrivilege` cuando corresponda.

La forma robusta de conseguirlo debe ser un **broker SST instalado como servicio LocalSystem**.

## TrustedInstaller

No debe formar parte de la ruta normal de:

```text
ps -> inspect -> suspend -> kill
```

TrustedInstaller puede reservarse para mantenimiento específico de:

- componentes protegidos de Windows;
- archivos cuyo owner sea `NT SERVICE\TrustedInstaller`;
- claves de registro protegidas.

No aporta una ventaja general para terminar procesos.

---

# 3. Vista de procesos

`ps` y `sys processes` deben mostrar los procesos como **árboles PID/PPID**, no como una lista plana.

Ejemplo:

```text
PID    PPID   USER       CPU   RAM    TRUST   PROCESS
3240   1812   usuario    2%    280M   signed  msedge.exe
├─3428 3240   usuario    0%    120M   signed  msedge.exe
├─5180 3240   usuario    1%     90M   signed  msedge.exe
│ └─7712 5180 usuario    0%     18M   ???     powershell.exe
│   └─8124 7712 usuario  0%      7M   unsigned update.exe
```

Datos mínimos por proceso:

- PID;
- PPID;
- usuario;
- SID;
- sesión;
- integrity level;
- CPU;
- RAM;
- nombre;
- ruta ejecutable;
- firma;
- publisher;
- edad del proceso;
- conexiones;
- protección PPL / Protected Process.

En `top`, CPU o RAM deben ordenar **hermanos dentro del mismo árbol**, sin destruir la jerarquía.

---

# 4. `sys inspect PID`

Debe ser uno de los comandos centrales.

```bash
sys inspect 8124
```

Salida esperada:

```text
Process
------------------------------------------------------------
PID:            8124
PPID:           7712
Name:           update.exe
Started:        2026-09-29 00:38:14
User:           DOMAIN\usuario
Integrity:      Medium
Session:        2

Executable
------------------------------------------------------------
Path:           C:\Users\usuario\AppData\Roaming\xyz\update.exe
SHA256:         ...
Signed:         no
Publisher:      -
Created:        ...
Modified:       ...

Parent
------------------------------------------------------------
powershell.exe
PID 7712
Command:
powershell.exe -nop -w hidden -enc ...

Network
------------------------------------------------------------
TCP  192.168.1.32:51221 -> 185.xxx.xxx.xxx:443
State: ESTABLISHED

Persistence
------------------------------------------------------------
HKCU\...\Run\Updater
Scheduled task: \UpdateTelemetry

Protection
------------------------------------------------------------
Protected process: no
PPL: no
Can query: yes
Can terminate: requires SYSTEM
```

---

# 5. `sys suspicious`

Debe mostrar sólo aquello que merece atención, sin declarar automáticamente que sea malware.

```bash
sys suspicious
```

Ejemplo:

```text
SUSPICIOUS  PID 8124  updater.exe

Reasons
------------------------------------------------------------
[+] Executable is unsigned
[+] First seen 3 minutes ago
[+] Located in AppData\Roaming
[+] Parent is powershell.exe
[+] Parent uses -EncodedCommand
[+] Outbound TCP connection
[-] No persistence detected
[-] No suspicious loaded modules detected
```

SST debe mostrar también evidencia que **reduce** la sospecha.

Ejemplo:

```text
ATTENTION  updater.exe

[+] Running from AppData
[+] Unsigned

[-] Present on this workstation for 420 days
[-] Same SHA-256 seen on 48 previous starts
[-] No network connections
[-] No persistence changes
[-] Parent is expected application
```

---

# 6. Niveles de atención

La interfaz debe usar pocos niveles:

```text
NORMAL
ATTENTION
SUSPICIOUS
HIGH
```

## NORMAL

No se detectó nada destacable.

No significa "seguro".

## ATTENTION

Algo poco habitual merece revisión.

## SUSPICIOUS

Varias señales independientes coinciden.

## HIGH

Existe una cadena de comportamiento con fuerte interés de seguridad.

SST no debe mostrar:

```text
MALWARE CONFIRMED
```

salvo que exista evidencia objetiva local, como un hash explícitamente bloqueado por el operador.

---

# 7. Evitar falsos positivos

Una sola señal no debe ser suficiente.

Estas señales son débiles de manera aislada:

- ejecutable sin firma;
- proceso en AppData;
- IP extranjera;
- PowerShell abierto;
- ejecutable nuevo;
- publisher desconocido;
- ventana oculta;
- CPU elevada.

La sospecha debe crecer con la **correlación**.

Ejemplo:

```text
powershell.exe
```

solo:

```text
INFO
PowerShell ejecutándose.
```

Pero:

```text
msedge.exe
  -> powershell.exe -EncodedCommand ...
```

debe generar:

```text
ATTENTION
PowerShell fue iniciado desde un navegador y usa contenido codificado.
```

Y:

```text
msedge.exe
  -> powershell.exe -enc ...
     -> update.exe
        path: AppData\Roaming\...
        unsigned
        outbound connection
        autorun created
```

puede escalar a:

```text
HIGH
Cadena de comportamiento anómala.
```

---

# 8. Señales

## Señales débiles

- unsigned;
- unknown publisher;
- ruta en AppData;
- ejecutable nuevo;
- IP externa;
- PowerShell;
- high CPU;
- hidden window.

## Señales medias

- browser -> PowerShell;
- Office -> script interpreter;
- encoded PowerShell;
- autorun inesperado;
- scheduled task nueva;
- servicio nuevo;
- DLL unsigned dentro de proceso firmado;
- ejecutable que se hace pasar por binario de Windows.

## Señales fuertes

Ejemplos:

```text
new executable
+ suspicious ancestry
+ persistence
+ outbound connection
```

o:

```text
signed host process
+ unsigned DLL loaded from user directory
+ outbound connection
```

---

# 9. Detección específica de cadenas ClickFix

SST debe reconocer como cadena de interés:

```text
Browser
  -> PowerShell / cmd
     -> download / file creation
        -> user-writable executable
           -> outbound connection
           -> persistence
```

Procesos/LOLBins de especial interés contextual:

- powershell.exe;
- pwsh.exe;
- cmd.exe;
- mshta.exe;
- rundll32.exe;
- regsvr32.exe;
- certutil.exe;
- bitsadmin.exe;
- wscript.exe;
- cscript.exe;
- schtasks.exe;
- wmic.exe;
- curl.exe.

No deben marcarse por existir, sino por **cómo fueron lanzados y qué hicieron después**.

---

# 10. PowerShell

SST debe prestar atención a patrones como:

- `-EncodedCommand`;
- `-enc`;
- `IEX`;
- `Invoke-Expression`;
- `DownloadString`;
- `DownloadFile`;
- `Invoke-WebRequest`;
- `Net.WebClient`;
- `Start-BitsTransfer`;
- hidden window;
- `-NoProfile`;
- `ExecutionPolicy Bypass`.

Ejemplo:

```text
HIGH powershell.exe PID 7712

Signals:
  - parent: msedge.exe
  - -WindowStyle Hidden
  - -EncodedCommand
  - spawned unsigned executable
  - executable subsequently opened external TCP connection
```

---

# 11. Baseline local

SST debe aprender qué es normal **en ese equipo**, no en Internet.

Base sugerida:

```text
data/baseline.db
```

Datos:

- SHA-256;
- path;
- publisher;
- parent;
- first_seen;
- last_seen;
- execution_count;
- usual_children;
- usual_network_destinations;
- usual_user.

Ejemplo relevante:

```text
first seen: today
seen: 1 time
```

es mucho más interesante que:

```text
first seen: 2024-01-12
seen: 18492 times
```

---

# 12. UNKNOWN no es SUSPICIOUS

SST debe distinguir explícitamente:

```text
UNKNOWN
```

de:

```text
SUSPICIOUS
```

Un binario recién instalado puede ser desconocido sólo porque SST nunca lo había visto.

---

# 13. Hashes

Todo ejecutable inspeccionado debe tener SHA-256.

La caché local debe registrar:

- hash;
- path;
- signature;
- publisher;
- first_seen;
- last_seen.

SST debe poder mostrar:

```text
FIRST SEEN TODAY
```

---

# 14. Firma Authenticode

Mostrar al menos:

```text
Signed: yes
Signature valid: yes
Publisher: Microsoft Corporation
```

o:

```text
Signed: no
```

Una firma válida reduce incertidumbre, pero **no anula otras señales**.

---

# 15. DLLs cargadas

Comando:

```bash
sys modules 8124
```

Ejemplo:

```text
C:\Windows\System32\kernel32.dll
C:\Windows\System32\ntdll.dll
C:\Users\...\AppData\Roaming\foo.dll    <- unsigned
```

Especialmente útil para detectar DLL sideloading.

---

# 16. Red

Comandos sugeridos:

```bash
sys connections
net processes
```

Mostrar:

- PID;
- proceso;
- local address;
- remote address;
- port;
- state;
- hostname, si se resuelve.

Ejemplo:

```text
PID   PROCESS       REMOTE                   STATE
8124  update.exe    185.x.x.x:443            ESTABLISHED
3200  msedge.exe    142.250.x.x:443          ESTABLISHED
```

País, ASN y organización pueden mostrarse como **contexto**, nunca como veredicto.

---

# 17. Persistencia

Comando:

```bash
sys persistence
```

Debe revisar al menos:

- HKCU Run;
- HKLM Run;
- RunOnce;
- Startup folders;
- Scheduled Tasks;
- Windows services;
- WMI permanent event subscriptions;
- Winlogon shell/userinit;
- AppInit_DLLs;
- IFEO debugger;
- browser extensions;
- PowerShell profiles.

---

# 18. Ejecutables recientes

Comando:

```bash
sys recent-executables --hours 24
```

Ejemplo:

```text
TIME        SIGNED PATH
00:31:35    no     C:\Users\...\AppData\Roaming\xyz\update.exe
```

---

# 19. Línea temporal

Comando:

```bash
sys timeline
```

Ejemplo:

```text
00:31:04  msedge.exe PID 3200 started
00:31:32  powershell.exe PID 7712 spawned by msedge.exe
00:31:32  PowerShell used encoded command
00:31:35  file created:
          C:\Users\...\AppData\Roaming\xyz\update.exe
00:31:36  update.exe PID 8124 started
00:31:37  outbound connection 185.xxx.xxx.xxx:443
00:31:42  registry persistence created
```

---

# 20. Preload de seguridad al arrancar SST

SST debe ejecutar un **preload ligero y no intrusivo** cada vez que arranca.

La finalidad no es "escanear el equipo completo" ni tomar acciones automáticas. Su objetivo es evitar que SST empiece completamente a ciegas frente a procesos, conexiones o mecanismos de persistencia que ya estaban activos antes de abrir la herramienta.

El preload debe responder rápidamente:

```text
¿Qué estaba vivo cuando SST arrancó?
¿Qué estaba conectado?
¿Qué apareció recientemente?
¿Qué merece que el operador lo mire?
```

## Principio operativo

El prompt y la interfaz deben estar disponibles cuanto antes.

El preload debe dividirse en dos fases:

```text
ARRANQUE
   |
   +-> FAST PRELOAD       lectura breve y prioritaria
   |      |
   |      +-> prompt usable
   |
   +-> BACKGROUND ENRICH  enriquecimiento progresivo
```

La primera fase debe tener un presupuesto de tiempo pequeño y predecible. No debe calcular hashes de todos los ejecutables, verificar todas las firmas ni recorrer todo el Registro antes de entregar la shell.

## Presupuesto de rendimiento

El preload no debe convertirse en parte pesada del arranque.

Objetivo:

```text
latencia añadida objetivo:   < 250 ms
latencia añadida aceptable:  < 500 ms
techo duro de bloqueo:        750 ms
```

Si una lectura no termina dentro del presupuesto, SST debe **deferirla al enriquecimiento en segundo plano** y entregar igualmente la shell.

El prompt no debe esperar a:

- hashing masivo;
- validación Authenticode de todos los procesos;
- resolución DNS;
- consultas WMI lentas;
- Event Log profundo;
- enumeración completa de DLLs;
- lectura extensa del Registro;
- análisis de handles;
- consultas remotas;
- geolocalización de IP;
- escaneo recursivo del filesystem.

El arranque debe privilegiar APIs nativas baratas y snapshots ya disponibles en memoria.

El preload debe ser **cancelable y degradable**:

```text
FAST PATH disponible
    -> captura mínima
    -> entrega prompt

fuente lenta / bloqueada
    -> timeout
    -> marca dato como pending/unavailable
    -> continúa en background
```

Nunca debe existir una operación individual capaz de bloquear indefinidamente el arranque.

## Caché y trabajo incremental

Para reducir costo:

- reutilizar el baseline local de la ejecución anterior;
- cachear hashes por `path + size + mtime`;
- cachear resultados Authenticode mientras el archivo no cambie;
- no volver a consultar metadatos estáticos de ejecutables conocidos;
- enriquecer primero procesos nuevos, raros o con señales previas;
- relegar procesos conocidos y estables al final de la cola;
- limitar concurrencia para no disparar CPU, disco o antivirus del host.

El enriquecimiento en background debe usar prioridad baja y ceder recursos ante actividad interactiva de SST.

## FAST PRELOAD

Debe capturar, como mínimo:

- snapshot completo de procesos PID/PPID;
- usuario/SID y sesión cuando estén disponibles;
- ruta del ejecutable;
- command line cuando pueda obtenerse de forma barata;
- procesos iniciados recientemente;
- conexiones TCP/UDP activas y PID asociado;
- servicios activos;
- tareas programadas de interés ya indexadas;
- entradas principales de persistencia;
- comparación inmediata con el baseline local;
- procesos que SST nunca había observado;
- relaciones padre/hijo especialmente relevantes;
- nivel de protección de procesos cuando pueda consultarse sin elevar.

El resultado se guarda como el **estado inicial de la sesión**.

## Enumeración defensiva de procesos

SST no debe depender únicamente de una lista visual o de una única fuente de enumeración.

La implementación debe poder contrastar, cuando sea viable:

- snapshot nativo de procesos;
- información PID/PPID del sistema;
- procesos observados por el sensor de eventos;
- conexiones de red que referencian PIDs;
- servicios con PID asociado.

Una discrepancia no significa automáticamente malware, pero sí debe convertirse en una señal:

```text
ATTENTION

PID 8124 aparece asociado a una conexión TCP,
pero no estaba presente en una de las vistas de procesos.

Reason:
process-enumeration discrepancy
```

Esto permite detectar anomalías que una vista superficial podría omitir.

SST no debe afirmar que puede descubrir un proceso oculto por un rootkit de kernel únicamente desde user mode. Si las distintas fuentes de Windows coinciden en ocultarlo, SST debe reconocer ese límite.

## BACKGROUND ENRICH

Después de entregar el prompt, SST puede completar progresivamente:

- SHA-256;
- Authenticode;
- publisher;
- first_seen / last_seen;
- frecuencia histórica;
- módulos cargados;
- correlación con persistencia;
- destinos de red conocidos para ese proceso;
- lineage más profundo;
- señales heurísticas adicionales.

Los resultados deben incorporarse al modelo de la sesión sin bloquear al usuario.

## Salida del preload

SST no debe imprimir una pared de información en cada arranque.

Si no encuentra nada destacable:

```text
preload: 168 processes · 42 connections · no notable anomalies
```

Si encuentra algo:

```text
preload: 171 processes · 45 connections · 2 items need attention

ATTENTION  powershell.exe [7712]
           browser parent + encoded command

ATTENTION  helper.exe [8124]
           first seen today + unsigned + outbound connection
```

El preload sólo debe llamar la atención. No debe suspender, terminar, bloquear red, eliminar persistencia ni poner archivos en cuarentena.

## Persistencia entre ejecuciones

Cada preload debe alimentar el baseline local:

- qué procesos existían al arrancar;
- hashes ya conocidos;
- rutas habituales;
- relaciones padre/hijo habituales;
- servicios habituales;
- destinos de red habituales;
- primeras y últimas apariciones.

Esto permite que la próxima ejecución de SST no parta de cero.

## Preload manual

Debe existir también:

```bash
triage preload
```

para repetir la lectura rápida sin reiniciar SST.

Opciones futuras:

```bash
triage preload --quiet
triage preload --details
triage preload --compare-last
```

## Regla de seguridad

El preload es **read-only**.

Nunca debe:

- matar;
- suspender;
- bloquear;
- modificar servicios;
- modificar tareas;
- borrar persistencia;
- cambiar ACL;
- elevar automáticamente a SYSTEM.

Si necesita datos que requieren privilegios superiores, debe marcar:

```text
additional inspection available with elevated privileges
```

y esperar una decisión explícita del operador.

---

# 21. Monitoreo en tiempo real

SST debe poder mantener un sensor ligero de eventos:

- process start;
- process exit;
- image/DLL load;
- network connection;
- registry persistence changes;
- scheduled task creation;
- service creation.

ETW debe ser la fuente preferida cuando resulte apropiado.

Esto permite conservar el PPID real incluso si el proceso padre desaparece después.

---

# 22. Husmear de forma progresiva

El análisis debe ser escalonado.

## Primera pasada

Barata y rápida:

- process tree;
- path;
- publisher;
- signature;
- parent;
- network;
- first_seen.

## Investigación

```bash
sys inspect PID
```

Debe añadir:

- command line;
- hash;
- modules;
- connections;
- persistence;
- service/task associations;
- event logs;
- file metadata.

## Investigación profunda

```bash
sys inspect PID --deep
```

Sólo bajo demanda:

- loaded modules;
- memory map;
- open handles;
- named pipes;
- DNS activity;
- recent file writes;
- registry modifications.

---

# 23. Respuesta manual

El flujo recomendado debe ser:

```text
inspect
  -> suspend
     -> capture evidence
        -> contain network
           -> kill
              -> remove persistence
                 -> quarantine
```

SST no debe ejecutar automáticamente esta secuencia.

---

# 24. Suspend / Resume

Antes de matar, debe existir:

```bash
sudo sys suspend 8124
sudo sys resume 8124
```

Suspender permite detener actividad potencialmente peligrosa sin perder el proceso inmediatamente.

---

# 25. Dump / evidencia

Comando:

```bash
sudo sys dump 8124
```

Salida sugerida:

```text
evidence\
2026-09-29_003800\
    process.json
    process.dmp
    executable.sha256
    modules.json
    network.json
    parent-tree.json
```

---

# 26. Terminación

Comandos:

```bash
sudo sys kill 8124
sudo sys kill 8124 --tree
sudo sys kill update.exe --all
```

SST debe verificar que los procesos efectivamente murieron.

Nunca debe asumir éxito sólo porque `TerminateProcess` devolvió éxito.

---

# 27. Procesos que reaparecen

Si un proceso reaparece, SST debe buscar:

- servicio;
- scheduled task;
- watchdog;
- proceso padre;
- otro mecanismo de persistencia.

Podría existir:

```bash
sudo sys kill update.exe --contain
```

que:

1. suspenda las instancias;
2. determine quién las relanza;
3. identifique el mecanismo de persistencia;
4. pida confirmación antes de neutralizarlo;
5. mate descendientes;
6. mate raíces;
7. confirme que no reaparecen.

No debe matar infinitamente a ciegas.

---

# 28. Quarantine

Comando:

```bash
sudo sys quarantine PID
```

Flujo:

- suspend;
- hash;
- copiar evidencia;
- kill;
- mover ejecutable;
- neutralizar persistencia sólo con confirmación.

Archivo:

```text
SHA256.bin.quarantined
```

---

# 29. Procesos críticos y PPL

SST debe distinguir claramente:

- normal;
- Protected Process;
- PPL-Windows;
- PPL-Antimalware;
- otros niveles soportados por Windows.

Procesos críticos del sistema no deben poder matarse por accidente.

Para acciones extremas se debe exigir un flag explícito, por ejemplo:

```bash
sudo sys kill PID --force-system-critical
```

SST no debe intentar burlar PPL.

---

# 30. Broker LocalSystem

Servicio sugerido:

```text
SSTPrivilegedBroker
```

Cuenta:

```text
LocalSystem
```

No debe aceptar ejecución arbitraria de comandos.

Operaciones permitidas:

```text
PROCESS_QUERY
PROCESS_SUSPEND
PROCESS_RESUME
PROCESS_TERMINATE
PROCESS_DUMP

FILE_HASH
FILE_QUARANTINE

NETWORK_BLOCK_PID

PERSISTENCE_DISABLE
```

---

# 31. Seguridad del broker

IPC sugerido:

```text
\\.\pipe\SSTPrivilegedBroker
```

El broker debe validar:

- PID cliente;
- SID cliente;
- sesión;
- usuario interactivo;
- integrity;
- pertenencia a Administrators cuando corresponda;
- hash/firma interna del binario SST cliente;
- origen exclusivamente local.

No debe existir una primitive genérica del tipo:

```text
RUN "cualquier comando como SYSTEM"
```

---

# 32. Snapshot de incidente

Comando:

```bash
incident snapshot
```

Debe recopilar:

- hostname;
- users;
- process tree;
- network;
- services;
- scheduled tasks;
- autoruns;
- recent executables;
- event logs;
- DNS cache;
- ARP/neighbors;
- routes;
- installed software;
- system time.

Destino:

```text
evidence\HOST_DATE\
```

---

# 33. Compare

Comando:

```bash
incident compare snapshot1 snapshot2
```

Debe mostrar:

- new process;
- new service;
- new scheduled task;
- new registry autorun;
- new executable;
- new outbound destination.

---

# 34. Watch mode

Comando:

```bash
incident watch
```

TUI sugerida:

```text
TIME      EVENT       PID     PROCESS
01:03:11  START       8124    update.exe
01:03:11  NETWORK     8124    -> 185.x.x.x:443
01:03:12  REGISTRY    8124    HKCU\...\Run
```

---

# 35. PowerShell history

Cuando exista, SST puede inspeccionar:

```text
%APPDATA%\Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt
```

También debe correlacionar Script Block Logging si está habilitado.

---

# 36. Portapapeles

Puede existir una función bajo demanda:

```bash
incident clipboard
```

Debe detectar contenido con patrones de interés como:

- powershell;
- mshta;
- curl;
- iex;
- certutil.

No debe vigilar ni bloquear permanentemente el clipboard salvo que exista una política explícita.

---

# 37. Allowlist

No confiar sólo por nombre.

Permitir confianza por:

- SHA-256;
- publisher;
- exact executable path.

Comandos conceptuales:

```bash
trust add HASH
trust add publisher
trust add path
```

Una firma válida no debe cancelar automáticamente otras señales.

---

# 38. Denylist local

Comando conceptual:

```bash
block hash SHA256
```

Si reaparece:

```text
KNOWN BLOCKED HASH
```

---

# 39. Threat intelligence externa

Debe ser opcional y desactivada por defecto.

SST no debe enviar automáticamente:

- hashes;
- nombres de archivos;
- información del host;
- procesos;
- direcciones;
- telemetría.

Un lookup externo debe ser siempre explícito.

---

# 40. Auditoría interna

Toda acción destructiva debe registrarse.

Ejemplo:

```text
2026-09-29T01:21:19
USER: DOMAIN\usuario
ACTION: PROCESS_TERMINATE_TREE
PID: 8124
IMAGE: update.exe
SHA256: ...
PRIVILEGE: SYSTEM
RESULT: success
```

Destino:

```text
data/audit/
```

---

# 41. Evidencia verificable

Snapshots y evidencia deben incluir:

```text
manifest.json
manifest.sha256
```

para detectar alteraciones posteriores.

---

# 42. Interfaz visual

Modos sugeridos:

```text
F2 Shell
F3 Processes
F4 Network
F5 Alerts
F6 Timeline
```

Vista de árbol:

```text
▼ msedge.exe
  ├─ msedge.exe
  ├─ msedge.exe
  └─ ⚠ powershell.exe
      └─ ● updater.exe
```

Panel de detalle:

```text
Why SST noticed this
------------------------------------------------------------
Browser spawned PowerShell
Encoded PowerShell command
Unsigned child executable
First observed 2 min ago

[Inspect]
[Suspend]
[Network]
[Persistence]
[Capture evidence]
[Kill]
[Quarantine]
```

No llenar la interfaz de rojo. El propósito es priorizar atención, no generar alarma.

---

# 43. Triage rápido

Comando:

```bash
triage
```

Ejemplo:

```text
SST TRIAGE
------------------------------------------------------------

Processes       173
New processes     4
Attention         2
Suspicious        1
High              0

Network
New outbound destinations: 3

Persistence
New entries: 1

Recent executable changes: 6
```

Luego:

```text
1. powershell.exe PID 8812
   ATTENTION
   browser parent + encoded command

2. helper.exe PID 9128
   SUSPICIOUS
   new unsigned binary + outbound connection
```

---

# 44. Regla central de UX

SST debe decir:

> **Esto es diferente o inusual, y éstas son las razones.**

No:

> **Esto es malware.**

La herramienta debe ayudar al operador a acotar el peligro sin sustituir su criterio.

---

# 45. Primera fase de implementación

La primera versión funcional debe cubrir:

1. preload rápido de seguridad al arrancar;
2. snapshot inicial read-only;
3. process tree PID/PPID;
4. `sys inspect PID`;
5. executable path;
6. command line;
7. owner/SID/integrity;
8. SHA-256;
9. Authenticode;
10. network connections by PID;
11. modules/DLLs;
12. protection/PPL;
13. LocalSystem broker;
14. suspend/resume;
15. kill/kill-tree;
16. verify termination;
17. persistence correlation;
18. suspicious signals;
19. timeline.

Con este bloque SST ya tendría valor real como herramienta de triage y respuesta local sin convertirse en un sistema autónomo de bloqueo o exterminio de procesos.
