use std::{collections::HashSet, sync::Arc, time::Duration};

use anyhow::Result;

use crate::core::{
    CommandContext, CommandOutput,
    ports::{TerminalFactory, TerminalKey, TerminalSession},
};

use super::{BuiltinCommand, UNIX_COMMANDS};

#[derive(Clone)]
struct WikiEntry {
    name: String,
    aliases: Vec<String>,
    category: String,
    summary: String,
    use_case: String,
    syntax: String,
    origin: String,
    compatibility: String,
}

fn entry(
    name: &str,
    aliases: &[&str],
    category: &str,
    summary: &str,
    use_case: &str,
    syntax: &str,
    origin: &str,
    compatibility: &str,
) -> WikiEntry {
    WikiEntry {
        name: name.to_owned(),
        aliases: aliases.iter().map(|value| (*value).to_owned()).collect(),
        category: category.to_owned(),
        summary: summary.to_owned(),
        use_case: use_case.to_owned(),
        syntax: syntax.to_owned(),
        origin: origin.to_owned(),
        compatibility: compatibility.to_owned(),
    }
}

fn wiki_entries() -> Vec<WikiEntry> {
    let mut rows = sst_entries();
    let mut seen = rows
        .iter()
        .map(|entry| entry.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    for name in BASH_BUILTINS {
        if seen.insert(name.to_ascii_lowercase()) {
            rows.push(bash_entry(name));
        }
    }

    for &(name, help) in UNIX_COMMANDS {
        if seen.insert(name.to_ascii_lowercase()) {
            rows.push(unix_entry(name, help));
        }
    }

    rows.sort_by(|left, right| {
        left.category
            .to_ascii_lowercase()
            .cmp(&right.category.to_ascii_lowercase())
            .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
    });
    rows
}

fn sst_entries() -> Vec<WikiEntry> {
    vec![
        entry(
            "notsh",
            &["net"],
            "Red",
            "Shell de red unificada de SST.",
            "Diagnóstico de interfaces, rutas, vecinos, sockets, DNS, descubrimiento y monitorización. Para Wi-Fi evita pedir ubicación cuando solo se necesita estado básico del adaptador.",
            "notsh SUBCOMANDO [opciones]\nnotsh wlan show interfaces\nnotsh lan show interfaces\nnotsh interfaces|routes|neighbors|connections\nnotsh scan [RED]\nnotsh monitor [RED]",
            "SST nativo; combina ideas de netsh, ip, iw, nmcli, ss, ethtool y resolvectl.",
            "No es un wrapper de netsh.exe para las consultas nativas implementadas por SST. 'net' es alias de compatibilidad.",
        ),
        entry(
            "device",
            &[],
            "Red",
            "Inventario local de equipos por MAC.",
            "Registrar equipos conocidos, consultar historial asociado y separar dispositivos conocidos/desconocidos.",
            "device list [--json|--csv]\ndevice show MAC|NOMBRE\ndevice add MAC|IP NOMBRE [--note TEXTO]\ndevice remove MAC\ndevice unknown [--json|--csv]\ndevice path",
            "SST nativo.",
            "Persistencia local de SST; puede resolver MAC desde IPv4 mediante vecino/ARP.",
        ),
        entry(
            "domain",
            &[],
            "Windows",
            "Consulta pertenencia a dominio del equipo local o remoto.",
            "Comprobar si una estación está unida a dominio y validar el estado reportado.",
            "domain status [EQUIPO] [--verify|--cim] [--json]",
            "SST nativo sobre APIs/probes de Windows.",
            "Lectura; --verify/--cim solicita validación adicional cuando está disponible.",
        ),
        entry(
            "switch",
            &[],
            "Red",
            "Resuelve MAC a switch y puerto.",
            "Localizar físicamente un equipo en infraestructura administrada mediante perfiles SNMP configurados.",
            "switch help\nswitch ...",
            "SST nativo sobre SNMP.",
            "Requiere perfiles/credenciales de switch configurados.",
        ),
        entry(
            "wol",
            &[],
            "Red",
            "Envía Wake-on-LAN.",
            "Encender un equipo compatible usando MAC o nombre del inventario SST.",
            "wol MAC|NOMBRE [opciones]",
            "Wake-on-LAN estándar.",
            "SST construye y envía el Magic Packet; depende de que firmware/NIC/red permitan WOL.",
        ),
        entry(
            "diag",
            &[],
            "Diagnóstico",
            "Ejecuta diagnósticos compuestos.",
            "Obtener una revisión rápida de red, hardware, almacenamiento y dominio sin lanzar cada comando por separado.",
            "diag [subcomando|opciones]",
            "SST nativo.",
            "Agrega servicios de diagnóstico internos de SST.",
        ),
        entry(
            "sys",
            &["ps", "top", "df", "free", "hostname", "whoami", "uname", "uptime", "fetch", "neofetch", "fastfetch"],
            "Sistema",
            "Información y administración local del sistema.",
            "Consultar procesos, memoria, discos, servicios, usuarios, impresoras, drivers, eventos, Registro y tareas programadas.",
            "sys info\nsys processes|top|disks|memory|uptime\nsys services [NOMBRE]\nsys users [USUARIO] [--domain]\nsys printers [--default]\nsys drivers\nsys events [LOG]\nsys registry CLAVE\nsys tasks [NOMBRE]",
            "SST nativo con APIs Windows y utilidades nativas cuando corresponde.",
            "Incluye aliases de estilo Unix; algunos subcomandos son de solo lectura y otros pasan por el broker/elevación.",
        ),
        entry(
            "triage",
            &[],
            "Seguridad",
            "Análisis activo de seguridad del equipo y procesos.",
            "Investigar procesos sospechosos, persistencia, relaciones y evidencias locales.",
            "triage [opciones|PID]",
            "SST nativo.",
            "Herramienta de triage; no sustituye un EDR ni un análisis forense completo.",
        ),
        entry(
            "intel",
            &[],
            "Seguridad",
            "Consulta estado, fuentes y lookups de inteligencia de seguridad.",
            "Correlacionar artefactos o indicadores con las fuentes configuradas en SST.",
            "intel [status|sources|lookup ...]",
            "SST nativo.",
            "La cobertura depende de las fuentes de inteligencia configuradas/disponibles.",
        ),
        entry(
            "instp",
            &[],
            "Paquetes",
            "Gestor de paquetes portables y aislados de SST.",
            "Descubrir e instalar herramientas dentro del entorno SST sin modificar el PATH global de Windows.",
            "instp\ninstp search [TEXTO]\ninstp info PAQUETE\ninstp install PAQUETE\ninstp remove PAQUETE\ninstp list\ninstp repo list|add|remove|update\ninstp doctor",
            "SST nativo; concepto similar a gestores de paquetes, con repositorios JSON propios.",
            "Los paquetes quedan aislados bajo SST. Paquetes system/driver no se instalan silenciosamente.",
        ),
        entry(
            "sudo",
            &["runas"],
            "Privilegios",
            "Eleva comandos o la sesión mediante mecanismos nativos de Windows.",
            "Ejecutar operaciones que requieren Administrador, SYSTEM o TrustedInstaller y revisar el token actual.",
            "sudo\nsudo --status\nsudo COMANDO [...]\nsudo --system COMANDO [...]\nsudo --trustedinstaller COMANDO [...]",
            "Inspirado en sudo; implementación SST sobre UAC/tokens Windows. 'runas' es alias y no ejecuta runas.exe.",
            "Escalera SST: usuario/admin filtrado -> Administrador -> SYSTEM -> TrustedInstaller.",
        ),
        entry(
            "eventlog",
            &[],
            "Windows",
            "Consulta y administra Windows Event Log.",
            "Listar logs/publicadores, leer eventos, exportar o limpiar un log.",
            "eventlog list|publishers\neventlog info LOG\neventlog read [LOG] [--count N] [--query XPATH] [--xml]\neventlog export LOG ARCHIVO [--query XPATH] [--overwrite]\neventlog clear LOG [--backup ARCHIVO]",
            "Hereda funcionalidad de wevtutil.exe.",
            "Las operaciones protegidas requieren elevación.",
        ),
        entry(
            "service",
            &[],
            "Windows",
            "Control de servicios de Windows.",
            "Consultar, iniciar, detener, pausar, reanudar o reiniciar servicios.",
            "service list [--running|--stopped]\nservice status NOMBRE\nservice start|stop|pause|resume|restart NOMBRE",
            "Hereda funcionalidad de sc.exe.",
            "Las mutaciones pueden requerir sudo/elevación.",
        ),
        entry(
            "registry",
            &[],
            "Windows",
            "Acceso al Registro de Windows.",
            "Consultar o modificar claves/valores y exportar/importar .reg.",
            "registry get CLAVE [--value NOMBRE|--default] [--recursive]\nregistry set CLAVE NOMBRE DATO [--type TIPO]\nregistry set-default CLAVE DATO [--type TIPO]\nregistry delete CLAVE [--value NOMBRE|--default|--key]\nregistry export CLAVE ARCHIVO [--overwrite]\nregistry import ARCHIVO",
            "Hereda funcionalidad de reg.exe.",
            "Las mutaciones pueden requerir sudo/elevación.",
        ),
        entry(
            "process",
            &[],
            "Windows",
            "Procesos Windows orientados a scripts Nwash.",
            "Listar procesos, consultar detalle/árbol o terminar un proceso.",
            "process list\nprocess info PID\nprocess tree PID\nprocess kill PID [--tree] [--force]",
            "Usa tasklist.exe/taskkill.exe y datos internos para el árbol.",
            "Procesos protegidos pueden requerir sudo.",
        ),
        entry(
            "acl",
            &[],
            "Windows",
            "Consulta y modifica ACL NTFS.",
            "Revisar o cambiar permisos de archivos y directorios.",
            "acl show RUTA\nacl grant RUTA USUARIO PERMISO\nacl deny RUTA USUARIO PERMISO\nacl revoke RUTA USUARIO\nacl inherit RUTA on|off\nacl reset RUTA",
            "Hereda funcionalidad de icacls.exe.",
            "Las modificaciones pueden requerir sudo.",
        ),
        entry(
            "pnp",
            &[],
            "Windows",
            "Gestiona dispositivos Plug and Play.",
            "Listar, inspeccionar, habilitar/deshabilitar/reiniciar dispositivos o solicitar un rescan.",
            "pnp list [--connected|--disconnected|--problem]\npnp info ID\npnp enable|disable|restart ID\npnp scan",
            "Hereda funcionalidad de herramientas PnP nativas de Windows.",
            "Las mutaciones pueden requerir elevación.",
        ),
        entry(
            "task",
            &[],
            "Windows",
            "Gestiona tareas programadas.",
            "Listar, consultar, ejecutar, terminar, habilitar o deshabilitar tareas.",
            "task list [--verbose|--csv]\ntask show NOMBRE [--xml]\ntask run|end|enable|disable NOMBRE",
            "Hereda funcionalidad de schtasks.exe.",
            "Las operaciones protegidas dependen de permisos del Programador de tareas.",
        ),
        entry(
            "session",
            &[],
            "Windows",
            "Consulta y gestiona sesiones locales/RDP.",
            "Ver usuarios/sesiones, cerrar sesión o enviar mensajes a una sesión.",
            "session list\nsession users\nsession logoff ID\nsession message ID TEXTO",
            "Hereda funcionalidad de utilidades de sesión Windows como query/quser, logoff y msg.",
            "Cerrar sesiones ajenas puede requerir elevación.",
        ),
        entry(
            "share",
            &[],
            "Windows",
            "Gestiona recursos compartidos SMB.",
            "Listar shares/sesiones/archivos abiertos y crear o eliminar un recurso compartido.",
            "share list\nshare sessions\nshare files\nshare add NOMBRE RUTA\nshare remove NOMBRE",
            "Hereda funcionalidad de net.exe y utilidades SMB de Windows.",
            "Crear/eliminar shares suele requerir elevación.",
        ),
        entry(
            "firewall",
            &[],
            "Windows",
            "Consulta y modifica Windows Defender Firewall.",
            "Ver perfiles/reglas y aplicar cambios de firewall desde Nwash.",
            "firewall status\nfirewall rules\nfirewall ...",
            "Hereda funcionalidad del firewall nativo de Windows.",
            "Los cambios de reglas requieren permisos suficientes.",
        ),
        entry(
            "power",
            &[],
            "Windows",
            "Apagado, reinicio, cierre de sesión e hibernación.",
            "Ejecutar acciones de energía desde scripts o la consola.",
            "power shutdown|restart|logoff|hibernate [opciones]",
            "Hereda mecanismos nativos de energía/sesión de Windows.",
            "Algunas acciones pueden requerir privilegios y son destructivas para la sesión actual.",
        ),
        entry(
            "helix",
            &["hx", "helix-sst"],
            "Editor",
            "Editor Helix integrado/adaptado para SST.",
            "Editar scripts, configuración y archivos directamente desde la consola.",
            "helix [ARCHIVO]\nhelix --help\nhelix --version\nhelix --credits",
            "Basado en Helix (helix-editor/helix), adaptado como helix-sst.",
            "No pretende ser un editor distinto: conserva la base Helix con integración SST.",
        ),
        entry(
            "sst-config",
            &[],
            "SST interno",
            "Backend de configuración portable de SST.",
            "Consultar/editar el archivo de configuración o gestionar fondos desde funciones de configuración.",
            "sst-config path|edit|bg ...",
            "SST nativo.",
            "Es backend; normalmente se usa mediante la función/builtin 'config'.",
        ),
        entry(
            "sst-path",
            &[],
            "SST interno",
            "Traduce rutas estilo Unix a rutas Windows.",
            "Resolver una ruta como /c/Users/... dentro del modelo de rutas de Nwash.",
            "sst-path PATH",
            "SST nativo.",
            "Auxiliar para compatibilidad de rutas.",
        ),
        entry(
            "tour",
            &[],
            "Ayuda",
            "Menú interactivo de ejemplos de Nwash/SST.",
            "Aprender funciones reales de la consola ejecutando scripts de examples/.",
            "tour",
            "SST nativo.",
            "Selecciona ejemplos empaquetados con SST.",
        ),
        entry(
            "config",
            &[],
            "Nwash",
            "Configura SST desde la shell.",
            "Editar/releer configuración y cambiar fondos sin salir de la consola.",
            "config [subcomando]\nconfig bg ...\nreload",
            "Builtin Nwash/SST.",
            "Extensión propia de Nwash; no pertenece a Bash estándar.",
        ),
        entry(
            "reload",
            &[],
            "Nwash",
            "Recarga la configuración de SST/Nwash.",
            "Aplicar cambios de configuración sin cerrar manualmente la consola cuando la opción lo permite.",
            "reload",
            "Builtin Nwash/SST.",
            "Extensión propia de Nwash.",
        ),
        entry(
            "wiki",
            &["manual"],
            "Ayuda",
            "Wiki interactiva local de comandos de SST.",
            "Descubrir qué comandos existen, cuándo se usan, su sintaxis y de qué herramienta heredan comportamiento.",
            "wiki\nwiki COMANDO\nwiki search TEXTO",
            "SST nativo.",
            "No necesita Internet y documenta también compatibilidad Bash/Unix.",
        ),
    ]
}

const BASH_BUILTINS: &[&str] = &[
    ":", ".", "[", "alias", "bg", "bind", "break", "builtin", "caller", "cd",
    "command", "compgen", "complete", "compopt", "continue", "declare", "dirs",
    "disown", "echo", "enable", "eval", "exec", "exit", "export", "false", "fc",
    "fg", "getopts", "hash", "help", "history", "jobs", "kill", "let", "local",
    "logout", "mapfile", "popd", "printf", "pushd", "pwd", "read", "readarray",
    "readonly", "return", "set", "shift", "shopt", "source", "suspend", "test",
    "times", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias",
    "unset", "wait",
];

fn bash_entry(name: &str) -> WikiEntry {
    let (syntax, summary, use_case) = bash_metadata(name);
    entry(
        name,
        if name == "source" { &["."] } else if name == "test" { &["["] } else { &[] },
        "Bash/Nwash",
        summary,
        use_case,
        syntax,
        "Builtin de Bash reproducido por el intérprete Nwash de SST.",
        "Compatibilidad orientada a Bash 5.3; SST implementa el comportamiento soportado por Nwash, no ejecuta bash.exe.",
    )
}

fn bash_metadata(name: &str) -> (&'static str, &'static str, &'static str) {
    match name {
        ":" => (":", "Builtin nulo que devuelve éxito.", "Usarlo como placeholder o en expansiones/ramas que no necesitan hacer nada."),
        "." | "source" => ("source ARCHIVO [ARGS]", "Ejecuta un archivo en la shell actual.", "Cargar funciones, variables o configuración sin abrir una subshell."),
        "[" | "test" => ("test EXPRESIÓN", "Evalúa expresiones condicionales.", "Comparar cadenas/números o probar propiedades de archivos en scripts."),
        "alias" => ("alias [NOMBRE[=VALOR] ...]", "Define o muestra aliases.", "Crear atajos de comandos para la sesión."),
        "unalias" => ("unalias [-a] NOMBRE ...", "Elimina aliases.", "Retirar atajos definidos en la shell."),
        "cd" => ("cd [DIR]", "Cambia el directorio actual.", "Moverse por el sistema de archivos; Nwash también acepta rutas Windows y /c/...."),
        "pwd" => ("pwd [-LP]", "Muestra el directorio actual.", "Confirmar la ruta de trabajo usada por comandos y scripts."),
        "echo" => ("echo [-neE] [ARG ...]", "Imprime argumentos.", "Mostrar texto o valores simples desde scripts."),
        "printf" => ("printf [-v VAR] FORMATO [ARG ...]", "Imprime con formato Bash.", "Generar salida con control preciso de formato o guardar en una variable."),
        "export" => ("export [-fn] [NOMBRE[=VALOR] ...]", "Exporta variables/funciones al entorno.", "Hacer variables visibles a procesos hijos."),
        "unset" => ("unset [-fv] NOMBRE ...", "Elimina variables o funciones.", "Limpiar estado de shell."),
        "readonly" => ("readonly [-aAf] [NOMBRE[=VALOR] ...]", "Marca bindings como solo lectura.", "Evitar cambios accidentales en variables o funciones."),
        "local" => ("local [OPCIONES] NOMBRE[=VALOR] ...", "Declara variables locales.", "Limitar variables al ámbito de una función."),
        "declare" | "typeset" => ("declare [OPCIONES] [NOMBRE[=VALOR] ...]", "Declara variables y atributos.", "Crear arrays, enteros, namerefs y otros atributos de Bash/Nwash."),
        "read" => ("read [OPCIONES] [NOMBRE ...]", "Lee entrada en variables.", "Pedir datos al usuario o consumir registros desde stdin."),
        "mapfile" | "readarray" => ("mapfile [OPCIONES] [ARRAY]", "Lee líneas en un array indexado.", "Cargar stdin/archivo en un array sin bucle manual."),
        "set" => ("set [OPCIONES] [--] [ARG ...]", "Configura opciones y parámetros posicionales.", "Controlar modos del shell y $1..$N."),
        "shopt" => ("shopt [-pqsu] [OPCIÓN ...]", "Configura opciones específicas de Bash.", "Activar compatibilidades/comportamientos de Nwash como autocd."),
        "shift" => ("shift [N]", "Desplaza parámetros posicionales.", "Procesar argumentos de scripts iterativamente."),
        "getopts" => ("getopts OPTSTRING NOMBRE [ARG ...]", "Parsea opciones cortas.", "Implementar flags tipo -a/-f en scripts."),
        "eval" => ("eval [ARG ...]", "Evalúa texto como código shell.", "Construir una orden dinámicamente; usar con cuidado por el riesgo de inyección."),
        "let" => ("let EXPRESIÓN ...", "Evalúa aritmética.", "Actualizar o comprobar expresiones enteras en sintaxis Bash."),
        "trap" => ("trap [-lp] [[ARG] SIGNAL ...]", "Gestiona traps/señales.", "Ejecutar limpieza o lógica al recibir señales/eventos del shell."),
        "break" => ("break [N]", "Sale de bucles.", "Interrumpir uno o varios niveles de loop."),
        "continue" => ("continue [N]", "Continúa la siguiente iteración.", "Saltar el resto de una iteración de loop."),
        "return" => ("return [N]", "Retorna de función o source.", "Terminar una función/archivo sourced con un estado."),
        "exit" => ("exit [N]", "Termina la shell o script.", "Salir de SST/Nwash o de un script con código de estado."),
        "logout" => ("logout [N]", "Sale de una shell de login.", "Cerrar una shell marcada como login."),
        "true" => ("true", "Devuelve estado 0.", "Usarlo en condiciones o loops que necesitan éxito constante."),
        "false" => ("false", "Devuelve estado 1.", "Usarlo en pruebas o control de flujo que necesita fallo constante."),
        "jobs" => ("jobs [OPCIONES] [JOB ...]", "Lista trabajos de la shell.", "Ver procesos/background jobs administrados por Nwash."),
        "fg" => ("fg [JOB]", "Trae un trabajo al foreground.", "Continuar interactivamente un job detenido/en background."),
        "bg" => ("bg [JOB ...]", "Reanuda jobs en background.", "Continuar trabajos detenidos sin bloquear la shell."),
        "wait" => ("wait [-fn] [ID ...]", "Espera procesos/jobs.", "Sincronizar scripts con procesos lanzados en background."),
        "disown" => ("disown [-arh] [JOB ...]", "Desasocia jobs de la shell.", "Evitar que un job siga administrado por la sesión actual."),
        "kill" => ("kill [-s SIGNAL|-SIGNAL] PID|JOB ...", "Envía señales a procesos/jobs.", "Terminar o señalizar procesos desde semántica Bash/Nwash."),
        "suspend" => ("suspend [-f]", "Suspende la shell.", "Detener temporalmente una shell cuando el host lo permite."),
        "history" => ("history [OPCIONES] [N]", "Consulta/modifica historial.", "Revisar o reutilizar comandos previos."),
        "fc" => ("fc [-e EDITOR] [-lnr] [PRIMERO [ÚLTIMO]]", "Edita/lista/repite historial.", "Reejecutar o editar órdenes previas al estilo Bash."),
        "bind" => ("bind [OPCIONES]", "Gestiona bindings de readline.", "Personalizar teclas/comportamiento de edición de línea."),
        "complete" => ("complete [OPCIONES] [NOMBRE ...]", "Define reglas de completado.", "Personalizar autocompletado de comandos."),
        "compgen" => ("compgen [OPCIONES] [PALABRA]", "Genera candidatos de completado.", "Inspeccionar o reutilizar el motor de completado."),
        "compopt" => ("compopt [OPCIONES] [NOMBRE ...]", "Ajusta opciones de completado.", "Modificar comportamiento de completados definidos."),
        "command" => ("command [-pVv] COMANDO [ARG ...]", "Ejecuta/resuelve un comando ignorando funciones cuando corresponde.", "Forzar resolución del comando real o inspeccionar cómo se resolvería."),
        "builtin" => ("builtin BUILTIN [ARG ...]", "Ejecuta explícitamente un builtin.", "Evitar que una función/alias con el mismo nombre lo oculte."),
        "type" => ("type [-afptP] NOMBRE ...", "Describe cómo se resuelve un nombre.", "Saber si algo es builtin, alias, función o ejecutable."),
        "hash" => ("hash [-lr] [-p RUTA] [NOMBRE ...]", "Gestiona caché de comandos.", "Consultar/actualizar rutas cacheadas de ejecutables."),
        "enable" => ("enable [-a] [-dnps] [-f ARCHIVO] [NOMBRE ...]", "Habilita/deshabilita builtins.", "Controlar qué builtins de Nwash pueden resolverse."),
        "exec" => ("exec [-cl] [-a NOMBRE] [COMANDO [ARG ...]]", "Reemplaza/ajusta ejecución de la shell.", "Ejecutar un comando como reemplazo del contexto actual o gestionar descriptores persistentes."),
        "umask" => ("umask [-pS] [MODO]", "Consulta/configura máscara de permisos.", "Controlar permisos por defecto dentro del modelo compatible de Nwash."),
        "ulimit" => ("ulimit [OPCIONES] [LÍMITE]", "Consulta/configura límites de shell.", "Compatibilidad de scripts Bash que consultan o cambian límites soportados."),
        "times" => ("times", "Muestra tiempos de CPU de shell/hijos.", "Medir consumo acumulado de CPU del proceso y descendientes."),
        "caller" => ("caller [N]", "Muestra contexto de llamada.", "Depurar funciones y archivos sourced."),
        "dirs" => ("dirs [-clpv] [+N|-N]", "Muestra la pila de directorios.", "Consultar rutas guardadas por pushd/popd."),
        "pushd" => ("pushd [-n] [DIR|+N|-N]", "Añade/rota pila de directorios.", "Navegar manteniendo una pila de ubicaciones."),
        "popd" => ("popd [-n] [+N|-N]", "Elimina una entrada de la pila.", "Volver a directorios guardados con pushd."),
        "help" => ("help [-dms] [PATRÓN ...]", "Muestra ayuda de builtins Nwash/Bash.", "Consultar sintaxis corta del intérprete; para documentación contextual usa guide."),
        _ => ("COMANDO [ARG ...]", "Builtin compatible de Nwash/Bash.", "Compatibilidad para scripts y uso interactivo de sintaxis Bash."),
    }
}

fn unix_entry(name: &str, help: &str) -> WikiEntry {
    let summary = help
        .split_once('—')
        .map(|(_, value)| value.trim())
        .unwrap_or(help);
    let (syntax, use_case) = unix_metadata(name);
    entry(
        name,
        &[],
        "Unix",
        summary,
        use_case,
        syntax,
        "Comando Unix/GNU-like reimplementado dentro de SST.",
        "Compatibilidad parcial e intencional: solo se documentan las opciones que Nwash/SST implementa; no ejecuta automáticamente el binario Unix original.",
    )
}

fn unix_metadata(name: &str) -> (&'static str, &'static str) {
    match name {
        "ls" => ("ls [-a] [-l] [RUTA]", "Listar archivos/directorios con salida simple o larga."),
        "cat" => ("cat [ARCHIVO ...]", "Concatenar archivos o pasar stdin a stdout."),
        "head" => ("head [-n N] [ARCHIVO]", "Ver las primeras líneas de texto."),
        "tail" => ("tail [-n N] [ARCHIVO]", "Ver las últimas líneas de texto."),
        "grep" => ("grep [-i] [-n] PATRÓN [ARCHIVO]", "Filtrar líneas por coincidencia de texto."),
        "wc" => ("wc [ARCHIVO]", "Contar líneas, palabras y bytes."),
        "sort" => ("sort [-r] [ARCHIVO]", "Ordenar líneas."),
        "uniq" => ("uniq [ARCHIVO]", "Eliminar duplicados adyacentes."),
        "cut" => ("cut [-d DELIM] [-f CAMPO] [ARCHIVO]", "Extraer un campo de cada línea."),
        "xargs" => ("xargs [OPCIONES] COMANDO [ARG ...]", "Construir ejecuciones de un comando a partir de stdin."),
        "tee" => ("tee [-a] ARCHIVO", "Copiar stdin a archivo y stdout."),
        "less" | "more" => ("less [ARCHIVO]", "Paginar texto de forma interactiva."),
        "sed" => ("sed 's/ANTIGUO/NUEVO/[g]' [ARCHIVO]", "Realizar sustituciones de texto soportadas por SST."),
        "awk" => ("awk PROGRAMA [ARCHIVO]", "Seleccionar/imprimir campos con el subconjunto soportado por SST."),
        "diff" => ("diff ARCHIVO1 ARCHIVO2", "Comparar dos archivos de texto."),
        "sha256sum" => ("sha256sum [ARCHIVO ...]", "Calcular SHA-256 para verificación de integridad."),
        "base64" => ("base64 [-d] [ARCHIVO]", "Codificar o decodificar Base64."),
        "find" => ("find [RUTA] [opciones soportadas]", "Buscar archivos en un árbol de directorios."),
        "basename" => ("basename RUTA", "Obtener el último componente de una ruta."),
        "dirname" => ("dirname RUTA", "Obtener el directorio padre textual de una ruta."),
        "realpath" => ("realpath RUTA", "Resolver una ruta absoluta en el modelo Windows/Nwash."),
        "date" => ("date [opciones soportadas]", "Mostrar fecha/hora desde scripts."),
        "sleep" => ("sleep DURACIÓN", "Pausar un script durante un intervalo."),
        "touch" => ("touch ARCHIVO ...", "Crear archivos vacíos o asegurar que existan."),
        "mkdir" => ("mkdir [-p] DIRECTORIO ...", "Crear directorios."),
        "rm" => ("rm [-r] [-f] RUTA ...", "Eliminar archivos/directorios; operación destructiva."),
        "cp" => ("cp [-r] ORIGEN DESTINO", "Copiar archivos/directorios."),
        "mv" => ("mv ORIGEN DESTINO", "Mover o renombrar archivos/directorios."),
        "tar" => ("tar [opciones soportadas] ARCHIVO [RUTAS...]", "Crear, listar o extraer tar/tar.gz."),
        "gzip" => ("gzip ARCHIVO ...", "Comprimir archivos con gzip."),
        "gunzip" => ("gunzip ARCHIVO.gz ...", "Descomprimir archivos gzip."),
        "zip" => ("zip ARCHIVO.zip RUTA ...", "Crear archivos ZIP."),
        "unzip" => ("unzip [-l] ARCHIVO.zip [DESTINO]", "Listar o extraer ZIP."),
        "which" => ("which COMANDO", "Localizar un ejecutable/comando visible en PATH."),
        _ => ("COMANDO [ARG ...]", "Usar la variante compatible implementada por SST."),
    }
}

fn categories(entries: &[WikiEntry]) -> Vec<String> {
    let mut rows = entries
        .iter()
        .map(|entry| entry.category.to_ascii_lowercase())
        .collect::<Vec<_>>();
    rows.sort();
    rows.dedup();
    rows.insert(0, "todos".to_owned());
    rows
}

fn visible_entries<'a>(entries: &'a [WikiEntry], query: &str, category: &str) -> Vec<&'a WikiEntry> {
    let query = query.trim().to_ascii_lowercase();
    let mut rows = entries
        .iter()
        .filter(|entry| category == "todos" || entry.category.eq_ignore_ascii_case(category))
        .filter(|entry| {
            query.is_empty()
                || entry.name.to_ascii_lowercase().contains(&query)
                || entry.aliases.iter().any(|alias| alias.to_ascii_lowercase().contains(&query))
                || entry.summary.to_ascii_lowercase().contains(&query)
                || entry.use_case.to_ascii_lowercase().contains(&query)
                || entry.syntax.to_ascii_lowercase().contains(&query)
                || entry.origin.to_ascii_lowercase().contains(&query)
                || entry.category.to_ascii_lowercase().contains(&query)
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()));
    rows
}

fn wrap(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut line = String::new();
    for word in value.split_whitespace() {
        let extra = if line.is_empty() { word.chars().count() } else { 1 + word.chars().count() };
        if !line.is_empty() && line.chars().count() + extra > width {
            rows.push(line);
            line = String::new();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() || rows.is_empty() {
        rows.push(line);
    }
    rows
}

fn clip(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut out = value.chars().take(width - 1).collect::<String>();
    out.push('…');
    out
}
