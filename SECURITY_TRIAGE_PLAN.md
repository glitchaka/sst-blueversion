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

## Caso de uso prioritario: degradación repentina del equipo

Un caso de uso central de SST es cuando el operador no sabe si existe un incidente de seguridad y sólo observa síntomas como:

- el equipo está repentinamente lento;
- abrir carpetas o archivos tarda mucho;
- aplicaciones que antes abrían rápido ahora demoran;
- el disco permanece ocupado;
- ventiladores/CPU trabajan sin una causa evidente;
- la RAM se consume de forma anormal;
- aparecen congelamientos o pausas breves;
- la red presenta actividad inesperada.

SST no debe asumir que esto significa malware.

Al arrancar debe poder distinguir rápidamente entre:

```text
DEGRADACIÓN EXPLICABLE
  proceso legítimo usando CPU
  presión de memoria
  I/O elevado
  antivirus escaneando
  Windows Update
  indexación
  aplicación conocida pesada

y

ANOMALÍA A REVISAR
  proceso nuevo o raro
  comportamiento distinto al histórico
  I/O inesperado
  parent/child inusual
  persistencia nueva
  conexión saliente no habitual
  reputación externa negativa
```

La salida debe ser breve y accionable.

Ejemplo normal:

```text
preload: system under load

Disk I/O:
  MsMpEng.exe        high
  SearchIndexer.exe  medium

No notable security anomalies.
```

Ejemplo que merece revisión:

```text
preload: system under load · 1 item needs attention

ATTENTION  helper.exe [8124]
           disk I/O: high
           first seen today
           parent differs from historical profile
           outbound connection present

Use: sys why 8124
```

El objetivo no es diagnosticar únicamente malware, sino responder:

> **"¿Qué está haciendo lento el equipo y hay algo en esa actividad que además sea inusual?"**

Esto permite que SST sea útil incluso cuando la causa termina siendo completamente legítima.

---

## Experiencia de arranque y presentación del preload

Al ejecutar SST, la terminal debe abrirse inmediatamente y mostrar el avance del preload de forma breve y legible mientras revisa el estado inicial del equipo.

No se debe usar un porcentaje artificial de progreso. Cada etapa puede tardar tiempos distintos y algunas pueden diferirse al background. SST debe mostrar **qué está revisando y en qué estado quedó**.

Estados visuales sugeridos:

```text
[·] revisando
[✓] completado
[~] diferido / pendiente
[!] requiere atención
[x] no disponible
```

Ejemplo durante el arranque:

```text
SST preload

[✓] procesos              173 encontrados
[✓] árbol PID/PPID        construido
[✓] CPU / RAM / I/O       revisado
[✓] conexiones            42 activas
[✓] inicio automático     18 entradas
[✓] servicios             96 activos
[✓] historial local       168 conocidos · 5 nuevos
[~] firmas / hashes       3 diferidos
[✓] inteligencia local    caché disponible
```

Las líneas deben actualizarse sin inundar el scroll de la terminal.

Cuando termina la fase rápida, SST debe mostrar exactamente:

> **«Hola. ¿Te gustaría destruir algún mal hoy?»**

Después del saludo, SST debe clasificar el resultado del preload en **sólo tres casos de uso**. No se intentará cubrir infinitos escenarios en el arranque.

---

### Caso 1 — Todo parece normal

No hay señales de seguridad relevantes.

SST debe mostrar:

1. un resumen de lo revisado;
2. procesos o servicios legítimos que estén consumiendo recursos de forma apreciable;
3. programas conocidos que se cargan al inicio y pueden afectar rendimiento;
4. comandos sugeridos para revisar rendimiento y arranque.

Ejemplo:

```text
«Hola. ¿Te gustaría destruir algún mal hoy?»

No encontré nada particularmente llamativo.

Revisado:
  procesos               173
  conexiones activas      42
  inicio automático       18
  servicios activos       96
  procesos nuevos          5
  elementos pendientes     3

Carga relevante:
  MsMpEng.exe            disk I/O high
  SearchIndexer.exe      disk I/O medium
  OneDrive.exe           startup + memory medium

Sugerencias:
  top --tree
  sys startup
  sys services --impact
```

El objetivo es poder responder también a:

```text
"el equipo está lento, pero no veo señales de compromiso"
```

#### Inicio automático y servicios legítimos

Aunque no existan sospechas de seguridad, SST debe revisar de forma ligera:

- programas de inicio del usuario;
- Run / RunOnce;
- Startup folders;
- servicios automáticos;
- procesos que aparecen inmediatamente después del inicio de sesión;
- impacto actual aproximado en CPU, RAM e I/O;
- historial de consumo cuando exista en SQLite.

Un programa firmado o conocido puede ser perfectamente legítimo y aun así perjudicar el rendimiento.

SST debe poder decir:

```text
PERFORMANCE

OneDrive.exe
  known / signed
  starts with user session
  memory: 620 MB
  disk I/O: medium

No security anomaly detected.
```

Esto es información de rendimiento, no una alerta de seguridad.

Comandos a implementar:

```bash
sys startup
sys services --impact
```

`sys startup` debe mostrar qué se carga al iniciar sesión o arrancar Windows y, cuando sea posible, su impacto actual/histórico.

`sys services --impact` debe priorizar servicios por consumo de recursos, sin sugerir deshabilitarlos automáticamente.

---

### Caso 2 — Hay algo que merece revisión

Existen una o más anomalías, pero la evidencia todavía es insuficiente para considerar que existe un incidente claro.

SST debe mostrar sólo los procesos relevantes y explicar entre 2 y 4 razones principales.

Ejemplo:

```text
«Hola. ¿Te gustaría destruir algún mal hoy?»

Encontré 2 procesos que merecen una mirada:

ATTENTION  powershell.exe [7712]
           parent inusual
           encoded command
           first seen in this context

ATTENTION  helper.exe [8124]
           first seen today
           outbound connection
           behavior differs from history

Sugerencias para revisar la sospecha:
  sys why 7712
  sys inspect 7712
  sys diff 8124
  intel lookup <sha256>

Sugerencias para revisar el equipo completo:
  triage
  sys suspicious
  sys startup
  sys services --impact
```

La segunda parte es importante: SST no debe conducir al operador únicamente hacia el proceso marcado.

También debe permitir comprobar si:

- existen otros procesos relacionados;
- la carga general explica el síntoma;
- el proceso marcado era un falso positivo;
- existe otra causa legítima de lentitud;
- hay una segunda anomalía que cambia el contexto.

En este caso SST **alerta e invita a verificar**, pero no recomienda todavía una acción destructiva.

---

### Caso 3 — Las alarmas se encienden

Es el caso menos frecuente.

Debe reservarse para evidencia fuerte o varias señales independientes que convergen, por ejemplo:

```text
hash exacto en blocklist / reputación externa
+ comportamiento local anómalo
```

o:

```text
proceso nuevo
+ lineage sospechoso
+ persistencia
+ conexión saliente
```

o:

```text
ejecutable históricamente conocido
+ comportamiento cambia de forma importante
+ nueva coincidencia de inteligencia externa
```

Ejemplo:

```text
«Hola. ¿Te gustaría destruir algún mal hoy?»

ALERT

helper.exe [8124]

Why:
  new executable in user-writable path
  spawned by encoded PowerShell
  outbound connection active
  persistence detected
  SHA-256 matched external reputation source

Sugerencias para profundizar:
  sys why 8124
  sys inspect 8124
  sys inspect 8124 --deep
  sys persistence
  intel lookup <sha256>

Si necesitas privilegios adicionales:
  sudo sys inspect 8124
  sudo sys suspend 8124

Acción destructiva sólo bajo decisión del operador:
  sudo sys kill 8124
  sudo sys kill 8124 --tree
```

El orden importa:

```text
entender
-> confirmar
-> escalar privilegios si hace falta
-> contener o terminar sólo si el operador lo decide
```

SST nunca debe matar, suspender o modificar persistencia automáticamente por entrar en este tercer caso.

---

### Reglas comunes a los tres casos

- El saludo siempre aparece después del FAST PRELOAD.
- No se muestran todos los procesos normales cuando existen alertas.
- `PENDING/YELLOW` significa análisis incompleto, no sospecha.
- Un proceso legítimo con alto consumo se muestra como **PERFORMANCE**, no como `ATTENTION`.
- `ATTENTION` significa "revísalo", no "es malware".
- El tercer caso requiere señales fuertes o múltiples señales independientes.
- Las sugerencias de comandos dependen del caso detectado.
- Los comandos sugeridos deben existir realmente en SST; si aún no están implementados, forman parte del alcance de implementación correspondiente.
- El preload no debe esperar consultas web ni análisis pesados.
- El enriquecimiento continúa en background después de entregar el prompt.

### Regla de rendimiento

La presentación del preload no cambia el presupuesto definido para el FAST PRELOAD.

Si una etapa excede su tiempo:

```text
[~] firmas / hashes       diferido
```

SST continúa con la siguiente.

La interfaz debe privilegiar siempre:

```text
mostrar estado -> clasificar en uno de 3 casos -> sugerir comandos -> entregar prompt -> seguir enriqueciendo en background
```

---

## Alcance y límites del módulo

Este módulo forma parte de SST y debe respetar el alcance original de la herramienta: **shell administrativa local con capacidad de observación, diagnóstico y acción manual**.

No debe intentar convertirse en:

- antivirus;
- EDR completo;
- sandbox;
- SIEM;
- motor forense integral;
- sistema autónomo de remediación;
- servicio permanente de vigilancia invasiva.

El núcleo de seguridad de SST debe limitarse a:

1. hacer una lectura rápida al arrancar;
2. mostrar procesos y relaciones relevantes;
3. detectar anomalías básicas y conocidas;
4. consultar historial local;
5. enriquecer con inteligencia externa cuando corresponda;
6. explicar por qué algo merece atención;
7. permitir inspección manual;
8. permitir suspensión/terminación manual cuando el operador lo decida.

Características como dumps completos de memoria, cuarentena automática, aislamiento de red por proceso, monitoreo permanente fuera de SST o recolección forense extensa quedan **fuera del núcleo** y sólo podrían considerarse posteriormente como módulos opcionales.

La prioridad es:

```text
bajo costo + alta visibilidad + explicación clara
```

no:

```text
máxima cobertura a cualquier costo
```

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

# 9. Motor de correlación y priorización

El **motor de correlación** es la pieza central del triage de SST.

Su función no es decidir si un proceso es malware. Su función es decidir:

~~~text
¿Hay algo aquí que merezca que el operador lo mire?
¿Por qué?
¿Con qué prioridad?
~~~

Debe combinar señales actuales, historial local e inteligencia externa sin convertir una sola señal débil en una alerta.

## Entradas

El motor puede recibir, según disponibilidad y costo:

~~~text
Proceso actual
  PID + creation/start time
  nombre
  path
  command line
  parent / children
  CPU
  RAM
  I/O
  conexiones
  inicio automático
  servicio asociado
  persistencia

Identidad
  SHA-256
  firma Authenticode
  publisher

Historial local
  first_seen
  last_seen
  execution_count
  parents habituales
  children habituales
  rutas habituales
  comportamiento de red habitual
  consumo habitual
  decisiones previas del operador
  estado de análisis previo

Inteligencia
  listas locales
  caché externa
  reputación por hash
  IOC
  contexto LOLBin / LOLScript
~~~

No todas las entradas estarán disponibles en el FAST PRELOAD.

El motor debe funcionar con información parcial y actualizar su conclusión cuando llegue información adicional desde la cola de background.

## Salidas

El motor debe producir una clasificación operacional:

~~~text
NORMAL
PERFORMANCE
ATTENTION
SUSPICIOUS
ALERT
~~~

### NORMAL

No se detectaron diferencias o señales relevantes con la información disponible.

No significa "seguro".

### PERFORMANCE

Existe consumo relevante de recursos que puede explicar lentitud, pero no hay señales de seguridad suficientes.

Ejemplo:

~~~text
PERFORMANCE
SearchIndexer.exe
disk I/O high
known process
expected parent
no unusual behavior detected
~~~

### ATTENTION

Existe una anomalía concreta que merece revisión, pero no hay evidencia suficiente para tratarla como incidente.

### SUSPICIOUS

Varias señales independientes convergen o existe una diferencia importante respecto del comportamiento histórico.

### ALERT

Existe evidencia fuerte o una combinación de señales de alta relevancia que justifica profundizar inmediatamente.

ALERT sigue sin significar "malware confirmado".

## Reglas de precedencia

El motor debe respetar estas reglas:

~~~text
historial conocido        != seguro
firma válida              != seguro
sin reputación externa    != seguro
sin firma                  != sospechoso por sí solo
alto consumo               != sospechoso por sí solo
ruta AppData               != sospechoso por sí solo
PowerShell                 != sospechoso por sí solo
PENDING                    != sospechoso
ACCESS_DENIED              != ausencia de evidencia
~~~

Y:

~~~text
señal fuerte actual
    > confianza histórica

varias señales independientes
    > una señal aislada

coincidencia exacta de SHA-256
    > coincidencia por nombre

cambio de comportamiento
    > mera antigüedad del proceso

evidencia local observable
    > inferencia por reputación genérica
~~~

## Independencia de señales

No se debe aumentar severidad simplemente por contar muchas señales que describen la misma cosa.

Ejemplo incorrecto:

~~~text
unsigned
unknown publisher
no trusted certificate
~~~

Estas tres pueden ser esencialmente la misma observación.

En cambio:

~~~text
unsigned
+ parent inusual
+ nueva persistencia
+ conexión saliente
~~~

sí representan señales independientes.

El motor debe agrupar señales por familias:

~~~text
IDENTITY
LINEAGE
EXECUTION
PERSISTENCE
NETWORK
RESOURCE_USAGE
HISTORY
EXTERNAL_INTEL
~~~

La severidad debe crecer principalmente cuando coinciden familias distintas.

## Confianza histórica

El historial local sirve para reducir ruido, no para blindar procesos.

Ejemplo:

~~~text
helper.exe
same hash
184 executions
usual parent: app.exe
usual network: none
~~~

Si hoy aparece:

~~~text
parent: powershell.exe
network: outbound
persistence: new
~~~

el resultado debe ser:

~~~text
ATTENTION o SUSPICIOUS

Reason:
historically known executable,
but current behavior differs from baseline.
~~~

Un proceso KNOWN_STABLE puede volver a revisión inmediatamente si cambia una señal importante.

## Información incompleta

Cada observación debe poder estar en estados como:

~~~text
KNOWN
UNKNOWN
PENDING
ACCESS_DENIED
UNAVAILABLE
~~~

El motor nunca debe interpretar UNKNOWN como FALSE.

Ejemplo:

~~~text
signature: ACCESS_DENIED
~~~

no equivale a:

~~~text
signature: invalid
~~~

## Inteligencia externa

La inteligencia externa modifica prioridad, pero no reemplaza el análisis local.

Ejemplos:

~~~text
LOCAL:
  no anomaly

EXTERNAL:
  exact SHA-256 match in blocklist

RESULT:
  ALERT
  reason: exact external hash match
~~~

~~~text
LOCAL:
  unusual parent + new outbound connection

EXTERNAL:
  no match

RESULT:
  ATTENTION
  local anomaly remains
~~~

~~~text
LOCAL:
  normal behavior

EXTERNAL:
  filename resembles known malware name

RESULT:
  no escalation by name alone
~~~

## Rendimiento y seguridad deben permanecer separados

El motor no debe convertir una causa de lentitud en una alerta de seguridad sólo por consumir recursos.

Ejemplo:

~~~text
MsMpEng.exe
disk I/O high
signed
known path
expected behavior
~~~

Resultado:

~~~text
PERFORMANCE
~~~

No ATTENTION.

Pero:

~~~text
helper.exe
disk I/O high
first seen today
parent: powershell.exe
outbound connection
~~~

puede resultar ATTENTION o SUSPICIOUS porque el consumo está acompañado de señales independientes.

## Explicabilidad

Toda clasificación distinta de NORMAL debe poder explicarse sin mostrar un score opaco.

El motor puede usar pesos internos para ordenar prioridades, pero la interfaz debe mostrar razones concretas.

Ejemplo:

~~~text
SUSPICIOUS  helper.exe [8124]

Why:
  first seen today
  parent differs from historical profile
  outbound connection present
  executable located in user-writable path
~~~

No mostrar un "Risk score: 87/100" como explicación principal.

## Relación con sys why

sys why PID debe ser la vista humana del resultado del motor de correlación.

Debe responder:

~~~text
Why SST noticed PID 8124

1. First appearance on this host
2. Parent differs from historical profile
3. Outbound connection is unusual for this executable
4. Similar behavior was previously marked WATCH
5. External reputation is pending
~~~

## Relación con sys diff

sys diff PID debe mostrar únicamente las diferencias entre el comportamiento actual y el perfil histórico relevante.

El motor puede usar ese diff como una de sus entradas.

## Relación con el preload

Durante el FAST PRELOAD:

~~~text
snapshot barato
    -> historial local
    -> reglas baratas
    -> clasificación inicial
~~~

Después:

~~~text
background enrichment
    -> hash / firma
    -> conexiones detalladas
    -> persistencia
    -> inteligencia externa
    -> recalcular clasificación si cambia la evidencia
~~~

Una clasificación puede cambiar durante la sesión:

~~~text
NORMAL -> ATTENTION
ATTENTION -> SUSPICIOUS
SUSPICIOUS -> ALERT
ATTENTION -> NORMAL
~~~

Toda transición relevante debe conservar la razón en el historial.

## Regla central

El motor debe optimizar para:

~~~text
detectar cambios relevantes
+ explicar por qué
+ minimizar falsos positivos
~~~

No para:

~~~text
marcar la mayor cantidad posible de cosas
~~~

---

# 10. Detección específica de cadenas ClickFix

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

# 11. PowerShell

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

# 12. Baseline local

SST debe aprender qué es normal **en ese equipo**, no en Internet.

Base sugerida:

```text
data/security.db
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

# 13. Base histórica local y memoria de comportamiento

SST necesita una **base de datos local persistente** para no empezar de cero en cada ejecución y para poder comparar el comportamiento actual con lo observado anteriormente.

La base recomendada es **SQLite**, almacenada localmente, sin dependencia de servidores externos.

Ruta sugerida:

```text
data/security.db
```

La base no debe guardar una conclusión simplista como:

```text
safe = true
```

Debe guardar evidencia histórica suficiente para responder:

```text
¿Ya vi este binario?
¿Con este mismo hash?
¿En esta misma ruta?
¿Firmado por el mismo publisher?
¿Normalmente lo lanza este mismo padre?
¿Suele abrir estas conexiones?
¿Suele crear estos hijos?
¿Cambió algo importante desde la última vez?
¿Un proceso parecido tuvo comportamiento anómalo anteriormente?
```

## Identidad de ejecutable

Cada ejecutable debe identificarse por una combinación de:

- SHA-256;
- ruta exacta;
- nombre;
- tamaño;
- fecha de modificación;
- firma Authenticode;
- publisher;
- versión de archivo;
- product name cuando exista.

El hash debe ser la identidad fuerte del contenido.

La ruta, nombre y publisher son contexto, no identidad suficiente por sí solos.

## Perfil de comportamiento

Para cada ejecutable observado, SST debe mantener un perfil histórico con:

- first_seen;
- last_seen;
- execution_count;
- usuarios habituales;
- sesiones habituales;
- padres habituales;
- hijos habituales;
- rutas habituales;
- command lines habituales o patrones normalizados;
- destinos de red habituales;
- puertos habituales;
- servicios asociados;
- tareas programadas asociadas;
- entradas de persistencia asociadas;
- DLLs habituales;
- integrity levels observados;
- firmas observadas;
- cambios de hash;
- decisiones manuales del operador;
- señales detectadas en ejecuciones anteriores.

## Estados históricos

No debe existir un estado absoluto de "seguro".

Estados sugeridos:

```text
NEW
KNOWN_STABLE
WATCH
SUSPICIOUS_HISTORY
BLOCKED
```

### NEW

Nunca visto antes o identidad materialmente distinta.

### KNOWN_STABLE

Observado repetidamente sin señales relevantes y con comportamiento estable.

Esto sólo permite **priorizarlo más abajo**, nunca ignorarlo completamente.

### WATCH

El operador o el motor decidió que merece seguimiento adicional.

### SUSPICIOUS_HISTORY

El ejecutable o un patrón muy similar mostró anteriormente señales relevantes.

### BLOCKED

Hash o identidad explícitamente bloqueada por el operador.

## Confianza con caducidad

`KNOWN_STABLE` no debe ser permanente.

La confianza histórica debe degradarse cuando:

- cambia el SHA-256;
- cambia la firma;
- cambia el publisher;
- cambia la ruta;
- cambia el proceso padre;
- aparecen hijos nuevos;
- cambia el command line;
- aparecen conexiones nuevas;
- aparece persistencia nueva;
- aumenta el nivel de privilegios;
- pasa demasiado tiempo desde la última observación.

Un proceso conocido puede volver automáticamente a:

```text
REVIEW_REQUIRED
```

si su comportamiento actual se aparta suficientemente del baseline.

## Revisión de procesos previamente considerados normales

Ejemplo:

```text
KNOWN_STABLE
helper.exe
hash: AAAAA...
usual parent: app.exe
usual network: none
```

Más adelante aparece:

```text
helper.exe
hash: AAAAA...
parent: powershell.exe
network: 185.x.x.x:443
persistence: HKCU Run
```

SST debe advertir:

```text
ATTENTION

helper.exe is historically known,
but current behavior differs from its normal profile.

Changed:
  parent: app.exe -> powershell.exe
  network: none -> outbound TCP
  persistence: none -> HKCU Run
```

El hecho de que el hash sea conocido **no elimina la alerta**.

## Similitud entre procesos

La base debe permitir detectar procesos distintos con comportamiento parecido.

Ejemplo:

```text
previous:
  abc123.exe
  parent: powershell.exe
  path: AppData
  unsigned
  outbound TCP
  Run-key persistence

current:
  updater02.exe
  parent: powershell.exe
  path: AppData
  unsigned
  outbound TCP
  Run-key persistence
```

Aunque el nombre y hash sean diferentes, SST debe poder decir:

```text
ATTENTION

Current process resembles a previously suspicious behavior pattern.
```

La comparación debe basarse en características, no sólo nombres.

## Huellas de comportamiento

SST puede generar una huella normalizada por ejecución, por ejemplo:

```text
parent_class
execution_path_class
signature_state
publisher
integrity
commandline_features
child_process_classes
network_destination_classes
persistence_types
loaded_module_classes
```

Estas huellas permiten comparar comportamientos sin convertir el sistema en una caja negra.

La interfaz siempre debe mostrar **qué rasgos coincidieron**.

## Aprendizaje conservador

La base histórica no debe "aprender confianza" demasiado rápido.

Un proceso no debe convertirse en `KNOWN_STABLE` sólo porque apareció una vez sin alertas.

Requisitos sugeridos:

- varias ejecuciones separadas en el tiempo;
- mismo hash;
- misma firma;
- mismo publisher;
- comportamiento consistente;
- ausencia de señales relevantes;
- sin cambios de persistencia inesperados.

El objetivo es evitar que malware recién llegado sea legitimado simplemente por haber sobrevivido una primera observación.

## Historial de decisiones del operador

Cuando el operador inspeccione algo, SST puede guardar:

```text
operator_action:
  reviewed
  trusted
  watch
  blocked
  false_positive
  quarantined
```

Pero una decisión humana tampoco debe borrar el historial técnico.

Ejemplo:

```text
operator: false_positive
technical history: preserved
```

Si el comportamiento cambia después, SST debe volver a advertir.

## Esquema lógico sugerido

Tablas mínimas:

```text
executables
process_instances
process_relationships
behavior_profiles
behavior_fingerprints
network_observations
persistence_observations
module_observations
signature_cache
hash_cache
alerts
operator_decisions
preload_sessions
event_timeline
```

### executables

Identidad relativamente estable del archivo.

Campos principales:

```text
id
sha256
path
name
size
mtime
signature_state
publisher
file_version
first_seen
last_seen
```

### process_instances

Cada ejecución concreta:

```text
id
executable_id
pid
ppid
user_sid
session_id
integrity
command_line
started_at
ended_at
preload_session_id
```

### behavior_profiles

Perfil agregado histórico:

```text
executable_id
execution_count
stability_state
last_reviewed
usual_parent
usual_integrity
usual_paths
usual_network_pattern
usual_children
```

### behavior_fingerprints

Huellas comparables entre ejecuciones:

```text
process_instance_id
fingerprint_version
features_json
```

Debe existir `fingerprint_version` para poder cambiar el algoritmo sin invalidar silenciosamente datos antiguos.

### operator_decisions

```text
timestamp
process_instance_id
executable_id
decision
reason
operator
```

## Índices

Para mantener el preload rápido, la base debe indexar como mínimo:

- SHA-256;
- path;
- publisher;
- executable_id;
- first_seen;
- last_seen;
- parent executable;
- remote endpoint;
- alert level.

Las consultas del preload deben ser simples y acotadas.

## Escrituras asíncronas

El preload no debe bloquearse esperando escrituras a SQLite.

Flujo:

```text
FAST PRELOAD
   -> resultados en memoria
   -> prompt disponible
   -> writer thread / queue
      -> SQLite
```

La escritura histórica debe realizarse en background mediante una cola.

## WAL

SQLite debe usar **WAL mode** para permitir lectura y escritura concurrentes con menor contención.

La shell debe poder consultar la base mientras el recolector escribe eventos.

## Retención

La base no debe crecer indefinidamente.

Política sugerida:

- conservar perfiles agregados a largo plazo;
- conservar alertas y decisiones del operador a largo plazo;
- conservar eventos detallados recientes durante una ventana configurable;
- compactar instancias antiguas a estadísticas;
- conservar hashes relevantes;
- conservar información asociada a incidentes explícitamente guardados.

La retención debe ser configurable.

## Privacidad

La base es local.

No debe sincronizarse ni enviarse automáticamente.

Puede contener información sensible como:

- nombres de usuario;
- command lines;
- rutas;
- IPs;
- procesos;
- historial de ejecución.

Debe almacenarse con permisos restringidos al usuario/administradores apropiados.

## Corrupción o pérdida de base

SST debe poder arrancar aunque `security.db` esté corrupta, bloqueada o ausente.

En ese caso:

```text
security history unavailable
running stateless preload
```

y continuar en modo temporal.

La base histórica nunca debe convertirse en un requisito para abrir la shell.

---

# 14. UNKNOWN no es SUSPICIOUS

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

# 15. Hashes

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

# 16. Firma Authenticode

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

# 17. DLLs cargadas

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

# 18. Red

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

# 19. Persistencia

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

# 20. Ejecutables recientes

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

# 21. Línea temporal

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

# 22. Preload de seguridad al arrancar SST

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

## Cola amarilla de análisis diferido

Si un proceso, archivo, firma, conexión o fuente de datos requiere más tiempo del permitido por el preload, SST **no debe quedarse esperando**.

Debe marcar ese elemento como pendiente y continuar inmediatamente con el siguiente.

Estado visual sugerido:

```text
YELLOW / PENDING
```

Esto no significa que el elemento sea sospechoso. Significa únicamente:

```text
SST encontró algo que merece completar,
pero su análisis excede el presupuesto del preload.
```

Ejemplo:

```text
YELLOW  PID 8124  helper.exe
        signature: pending
        hash: pending
        modules: deferred
        reason: preload time budget exceeded
```

Reglas:

- cada operación del preload debe tener timeout propio;
- al excederlo, el elemento pasa a la cola amarilla;
- SST continúa con el siguiente proceso sin bloquear;
- la cola amarilla se procesa después en background;
- los elementos amarillos no deben elevarse automáticamente a `ATTENTION`, `SUSPICIOUS` o `HIGH`;
- si el enriquecimiento posterior encuentra señales reales, recién entonces cambia su nivel;
- si el análisis termina sin anomalías, el elemento vuelve silenciosamente a estado normal;
- el operador puede inspeccionar manualmente cualquier elemento pendiente sin esperar al background.

La cola debe priorizar:

1. procesos nuevos;
2. procesos con parent/child inusual;
3. ejecutables en rutas escribibles por usuario;
4. procesos con conexiones salientes activas;
5. elementos que ya tenían señales previas;
6. procesos conocidos y estables al final.

El objetivo es que SST prefiera **cobertura amplia y rápida** antes que atascarse intentando resolver exhaustivamente un único elemento.

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
- CPU, RAM e I/O de disco por proceso mediante lecturas baratas;
- presión global de CPU, memoria y disco;
- servicios activos y su consumo cuando pueda obtenerse de forma barata;
- programas de inicio automático ya indexados y su impacto aproximado;
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

# 23. Monitoreo en tiempo real

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

# 24. Husmear de forma progresiva

El análisis debe ser escalonado.

## Primera pasada

Barata y rápida:

- process tree;
- CPU/RAM/I/O;
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

# 25. Respuesta manual

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

# 26. Suspend / Resume

Antes de matar, debe existir:

```bash
sudo sys suspend 8124
sudo sys resume 8124
```

Suspender permite detener actividad potencialmente peligrosa sin perder el proceso inmediatamente.

---

# 27. Dump / evidencia

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

# 28. Terminación

Comandos:

```bash
sudo sys kill 8124
sudo sys kill 8124 --tree
sudo sys kill update.exe --all
```

SST debe verificar que los procesos efectivamente murieron.

Nunca debe asumir éxito sólo porque `TerminateProcess` devolvió éxito.

---

# 29. Procesos que reaparecen

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

# 30. Quarantine

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

# 31. Procesos críticos y PPL

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

# 32. Broker LocalSystem

Servicio:

```text
SSTPrivilegedBroker
```

Cuenta:

```text
LocalSystem
```

El broker de la primera versión funcional es deliberadamente pequeño. No acepta
ejecución arbitraria de comandos ni amplía el lenguaje del shell.

Contrato v2 implementado:

```text
INSPECT
SUSPEND
RESUME
KILL
```

`INSPECT` puede comenzar sólo con PID. El broker abre el objetivo una vez,
obtiene su FILETIME exacto y devuelve la identidad estable `PID + FILETIME`.

Toda mutación exige esa identidad exacta obtenida previamente:

```text
PID + FILETIME
```

Si el PID fue reutilizado o la creación no coincide, la operación se rechaza.

Quedan fuera del protocolo de este release:

```text
PROCESS_DUMP
FILE_QUARANTINE
NETWORK_BLOCK_PID
PERSISTENCE_DISABLE
RUN / ejecución arbitraria
```

Esas capacidades no deben aparecer accidentalmente como extensiones genéricas
del broker. Si alguna se incorpora en una versión futura, debe añadirse como
operación tipada y revisarse de manera independiente.

---

# 33. Seguridad del broker

IPC:

```text
\\.\pipe\SSTPrivilegedBroker
```

El broker debe validar:

- PID y FILETIME exacto del cliente;
- SID cliente configurado;
- sesión interactiva;
- integrity mínimo Medium;
- ausencia de AppContainer;
- AuthenticationId y coincidencia con el token primario;
- pertenencia a Administrators + token elevado para mutaciones;
- identidad del servidor contra SCM;
- cuenta LocalSystem;
- misma imagen SST protegida y mismo SHA-256;
- origen exclusivamente local;
- nonce por petición y rechazo de replay;
- tamaño máximo de frame;
- timeout de I/O y ACK;
- estado crítico/PPL antes de mutar;
- identidad exacta del objetivo antes de actuar.

No debe existir una primitive genérica del tipo:

```text
RUN "cualquier comando como SYSTEM"
```

El helper `--broker-client` tampoco carga configuración, RC, plugins o shell.
Su única función es construir una petición tipada y hablar con el servicio.

---

# 34. Snapshot de incidente

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

# 35. Compare

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

# 36. Watch mode

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

# 37. PowerShell history

Cuando exista, SST puede inspeccionar:

```text
%APPDATA%\Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt
```

También debe correlacionar Script Block Logging si está habilitado.

---

# 38. Portapapeles

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

# 39. Allowlist

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

# 40. Denylist local

Comando conceptual:

```bash
block hash SHA256
```

Si reaparece:

```text
KNOWN BLOCKED HASH
```

---

# 41. Inteligencia externa y reputación

SST debe poder consultar **fuentes públicas o privadas de inteligencia de seguridad** para enriquecer una observación local.

Esto es especialmente útil cuando un hash, URL, dominio, IP, certificado o patrón de ejecución ya ha sido identificado previamente por comunidades o proveedores de seguridad.

La inteligencia externa es una **señal adicional**, no un veredicto automático.

## Qué consultar

Orden de valor recomendado:

1. SHA-256 del ejecutable;
2. certificado / publisher;
3. dominio o URL observada;
4. IP:puerto;
5. patrón LOLBin / LOLScript;
6. nombre de archivo o proceso, sólo como señal débil.

Un nombre como `update.exe` o `svchost.exe` nunca debe considerarse suficiente para declarar algo sospechoso.

## Fuentes configurables

Las fuentes no deben quedar hardcodeadas en Rust.

SST debe cargar un archivo editable:

```text
data/security.sources
```

Este archivo es **registro de fuentes de inteligencia**, no configuración general de SST; `config/sstrc` sigue siendo el único archivo principal de configuración de la aplicación.

Cada fuente debe poder definir:

```text
id
enabled
kind
base_url
lookup
auth
timeout_ms
cache_ttl
priority
notes
```

Si una fuente desaparece, cambia API, exige autenticación o deja de ser útil, debe poder deshabilitarse o reemplazarse sin recompilar SST.

## Actualización

SST debe mantener una caché local de reputación en SQLite.

Flujo:

```text
observación local
    -> buscar caché
        -> dato vigente: usar
        -> dato vencido/desconocido:
             consultar fuentes habilitadas
             actualizar caché
```

No debe consultar Internet durante el FAST PRELOAD.

Las consultas externas se ejecutan:

- en background;
- bajo demanda con `sys inspect`;
- o mediante actualización manual.

Comandos previstos:

```bash
intel update
intel status
intel lookup SHA256
intel sources
```

## Resiliencia

Una fuente caída nunca debe bloquear SST.

Estados de fuente:

```text
OK
STALE
UNAVAILABLE
AUTH_REQUIRED
DISABLED
INVALID_RESPONSE
```

Si todas fallan:

```text
external reputation unavailable
local analysis continues
```

## Fuentes iniciales razonables

El registro inicial puede incluir fuentes como:

- MalwareBazaar, para reputación/metadata por hash;
- ThreatFox, para IOC conocidos;
- URLhaus, para URLs relacionadas con malware;
- LOLBAS, para contexto sobre binarios y scripts legítimos susceptibles de abuso.

Estas fuentes tienen propósitos diferentes y no deben mezclarse como si todas afirmaran "malware".

MalwareBazaar y ThreatFox actualmente requieren Auth-Key para sus APIs comunitarias; SST no debe almacenar claves directamente en el archivo de fuentes. El registro debe referenciar una variable de entorno o un secreto local separado. Las APIs y condiciones de uso pueden cambiar, por lo que el archivo editable es deliberadamente parte del diseño. 

## Correlación con historial local

La inteligencia externa debe integrarse con la base histórica:

```text
LOCAL:
  known_stable for 180 days

EXTERNAL:
  hash newly reported malicious

RESULT:
  HIGH ATTENTION
  "Previously stable executable now has an external malicious-hash match."
```

Y también al revés:

```text
LOCAL:
  unusual parent + new outbound connection

EXTERNAL:
  no matches

RESULT:
  ATTENTION
  "No external reputation match; local anomaly remains."
```

Ausencia en fuentes externas **no significa seguro**.

---



# 42. Auditoría interna

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

# 43. Evidencia verificable

Snapshots y evidencia deben incluir:

```text
manifest.json
manifest.sha256
```

para detectar alteraciones posteriores.

---

# 44. Interfaz visual

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

# 45. Triage rápido

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

# 46. Regla central de UX

SST debe decir:

> **Esto es diferente o inusual, y éstas son las razones.**

No:

> **Esto es malware.**

La herramienta debe ayudar al operador a acotar el peligro sin sustituir su criterio.

---

# 47. Primera fase de implementación

La primera versión funcional debe mantenerse dentro del scope original de SST:

1. preload rápido y read-only;
2. process tree PID/PPID con identidad estable de instancia;
3. `sys inspect PID` básico;
4. path, command line, owner/SID/integrity;
5. conexiones por PID;
6. SHA-256 y Authenticode bajo demanda/background;
7. SQLite histórico local;
8. comparación contra comportamiento previo;
9. señales explicables `ATTENTION / SUSPICIOUS / HIGH`;
10. reputación externa configurable y cacheada;
11. `intel update / status / lookup`;
12. suspensión y kill/kill-tree manual;
13. verificación de terminación;
14. broker LocalSystem únicamente para operaciones que realmente lo requieran.

Quedan fuera de esta primera fase:

- cuarentena automática;
- dumps completos;
- aislamiento de red por PID;
- sensor residente permanente;
- análisis profundo de memoria;
- vigilancia del portapapeles;
- framework forense completo;
- remediación automática de persistencia.


Con este bloque SST ya tendría valor real como herramienta de triage y respuesta local sin convertirse en un sistema autónomo de bloqueo o exterminio de procesos.
