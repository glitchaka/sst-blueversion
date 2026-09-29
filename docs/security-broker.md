# Broker LocalSystem

Implementación del broker de procesos de las secciones 32–33 del plan. Servicio
`SSTPrivilegedBroker`, cuenta LocalSystem, pipe local `\\.\pipe\SSTPrivilegedBroker`.
Su punto de entrada se ejecuta antes de la GUI, el intérprete y la configuración
del usuario. El contrato sólo admite `INSPECT`, `SUSPEND`, `RESUME` y `KILL`.
No admite comandos, scripts, rutas, variables de entorno, nombres de proceso,
árboles, ejecución de programas ni extensiones proporcionadas por el cliente.

## Autorización

La instalación configura un SID concreto de operador. La ACL del pipe permite
a ese SID leer/escribir datos, sin `FILE_CREATE_PIPE_INSTANCE`. Es una distinción
necesaria porque los permisos genéricos de escritura incluyen creación de
instancias. [Documentación de Microsoft](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights).

Después de leer un mensaje acotado, el servidor obtiene PID/sesión del pipe e
impersona al cliente para consultar su token. Comprueba SID configurado, sesión
interactiva no cero, integridad mínima Medium, ausencia de AppContainer y
coincidencia con el token primario del proceso: SID, sesión, AuthenticationId,
integridad y elevación. Siempre revierte la impersonación antes de operar.
Si la reversión falla, el proceso aborta; no continúa con identidad indeterminada.

La política efectiva es:

| Cliente autorizado | Inspect | Suspend / resume / kill |
| --- | --- | --- |
| Usuario normal / administrador filtrado | Sólo su SID y sesión | Rechazado |
| Administrador elevado | Otros usuarios/sesiones, sujeto a permisos de Windows | Permitido salvo restricciones de protección |

Las mutaciones requieren **ambos**: Administrators habilitado en el token y
elevación con integridad High o superior. Conocer el SID, un PID o el nombre
del pipe no autoriza una operación. Elevación con credenciales de otro SID no
coincide con el operador configurado y se rechaza.

## Identidades y límites

- El cliente autentica al servidor contra SCM: servicio propio en ejecución,
  PID publicado y cuenta LocalSystem. Conserva abierto el handle del servidor.
- Ambos extremos deben corresponder a `sst.exe` instalado en Program Files y
  al mismo SHA-256. Se validan propietario y ACL del directorio/ejecutable;
  se rechazan escritores distintos de SYSTEM y Administrators. Los archivos
  permanecen abiertos sin compartir escritura/borrado durante su validación.
- Cada petición contiene versión, nonce aleatorio de 128 bits, PID/creación del
  cliente, sesión, operación y PID/creación del objetivo. El nonce no sustituye
  la autenticación. Se rechazan nonces repetidos durante la vida del servicio.
- La creación es el **FILETIME exacto de Windows**, no segundos Unix ni el valor
  truncado de `sysinfo`. El broker abre una sola vez el objetivo, valida la
  identidad y usa ese mismo handle hasta terminar la operación.
- Una respuesta debe coincidir en versión, nonce, operación e identidad y tener
  el tipo de resultado correspondiente. Se rechazan campos/variantes desconocidos,
  frames mayores de 8 KiB y contenido textual de respuesta inválido.
- El pipe rechaza clientes remotos y exige la primera instancia. Las lecturas,
  escrituras y ACK tienen plazos; una I/O cancelada se drena antes de liberar
  sus buffers. Una conexión procesa exactamente una petición.
- No hay reintento automático de mutaciones. Si se pierde la respuesta, el
  resultado puede ser desconocido: consultar estado/auditoría antes de repetir.
- Críticos, PPL/Protected Process, procesos centrales y el propio broker se
  rechazan para mutación, sin `--force` ni intento de eludir protección. Si no
  puede determinarse estado crítico/protección, se rechaza la operación.

La comprobación del archivo no convierte SST en un proceso protegido contra
inyección por el mismo usuario. El límite de privilegios sigue siendo el token:
un usuario sin elevar no obtiene operaciones administrativas usando el helper.
Un administrador local ya controla el servicio y queda fuera de ese límite.

## Integración con SST portable

La GUI portable lanza un helper fijo de la instalación protegida, con el mismo
token y argumentos cerrados. Ese helper entra por `--broker-client`, sin leer
RC/configuración ni inicializar el shell. Sólo él habla con el servicio.
La GUI no pasa a ejecutarse como SYSTEM. El helper no realiza UAC por su cuenta.

`INSPECT` es la única operación que puede comenzar sólo con PID. El broker abre
el objetivo una vez, obtiene su FILETIME exacto y lo devuelve como identidad.
Esto permite usar el broker precisamente cuando el proceso no puede inspeccionarse
desde el token normal. `SUSPEND`, `RESUME` y `KILL` siguen exigiendo el
FILETIME exacto devuelto por una inspección previa.

```bash
sys inspect 8124 --broker
# La salida incluye "Start time (FILETIME)". Copiar ese valor exacto:
sudo sys suspend 8124 --start-time 134000000000000000
sudo sys resume 8124 --start-time 134000000000000000
sudo sys kill 8124 --broker --start-time 134000000000000000
```

El número anterior es sólo un ejemplo. Las mutaciones exigen `--start-time`;
no recapturan la identidad después de UAC. El inspect inicial puede enviar sólo
el PID: el servicio resuelve y devuelve la identidad exacta mientras conserva
abierto el handle del objetivo.

`sys inspect` y `sys kill` sin `--broker` conservan su implementación local.
Los mecanismos antiguos de `sudo --system`/TrustedInstaller son independientes
y no forman parte del protocolo ni de las garantías de este broker.

## Instalación explícita

El código no instala ni inicia el servicio automáticamente. El script requiere
PowerShell elevado, el SID del operador y el SHA-256 explícitamente aprobado del
ejecutable compilado. Rechaza instalaciones existentes, no sobrescribe binarios
y verifica el hash también después de copiarlos.

```powershell
.\scripts\install-security-broker.ps1 -Executable 'C:\artefactos\sst.exe' `
    -OperatorSid 'S-1-5-21-111-222-333-1001' -ExpectedSha256 '<64 hex>'
# Queda instalado y detenido. Activación explícita posterior:
Start-Service SSTPrivilegedBroker
```

Se instala en `%ProgramFiles%\SSTPrivilegedBroker`, propietario Administrators,
ACL protegida; usuarios sólo leen/ejecutan. La auditoría sólo es accesible a
SYSTEM/Administrators. Usuarios interactivos pueden consultar la identidad SCM,
pero no iniciar, detener ni reconfigurar el servicio. SCM limita los privilegios
a SeChangeNotifyPrivilege, SeImpersonatePrivilege y SeDebugPrivilege.

## Auditoría y operación

`broker-audit.jsonl` registra inicio/parada, rechazos de conexión, intención
autenticada y resultado. La intención se sincroniza a disco **antes** de actuar.
Si no se puede auditar, no se inicia una nueva acción. Una falla posterior a la
acción puede impedir confirmar el resultado al cliente; no implica rollback.

La auditoría tiene límite de 16 MiB y no sobrescribe historia. Al agotarse debe
archivarse con el servicio detenido y recrearse con la misma ACL. El conjunto
antirrepetición admite hasta 65.536 peticiones por arranque; agotarlo exige un
reinicio administrativo. Un operador autorizado puede agotar disponibilidad,
pero no convertir ese agotamiento en una autorización adicional.

Suspend/resume usan exclusivamente las dos funciones correspondientes de ntdll,
si están disponibles. Conservan la semántica nativa de suspensión: no son
operaciones idempotentes y no debe repetirse una petición cuyo resultado sea
incierto. Reiniciar el broker no reanuda automáticamente procesos suspendidos.

Estado del release: implementación funcional cerrada en código y protocolo v2.
La compilación, instalación y prueba real del servicio siguen siendo la validación
final separada; no se ejecutaron durante esta implementación.
