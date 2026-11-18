# SST-PACKAGE/1 — estándar de paquetes de Shell Shock Tool

Estado: contrato de catálogo y ejecución **v1**. El índice de repositorios emplea `schema: 2`; cada entrada declara `format: 1` y `execution`. El formato no equivale a los formatos externos de los proveedores (.zip, .tar.gz, .wsl).

## Objetivo y límites

`instp` instala, verifica, registra, actualiza y desinstala. Nwash conserva la consola y enruta comandos, scripts y E/S al adaptador registrado. El adaptador interpreta o ejecuta; el catálogo **no** implementa por sí solo un nuevo lenguaje.

Las clases no son etiquetas intercambiables:
- `embedded`: ensamblado/plug-in cargado **en el proceso SST**, con ABI versionada (ej. PowerShell SDK + hostfxr).
- `runtime`: intérprete externo administrado (Python, Ruby, Deno, Bun), ligado a extensiones de scripts. Puede crear un proceso hijo, sin abrir otra consola.
- `process`: herramienta CLI nativa (rg, fd, jq, bat, etc.). SST mantiene su E/S y controla el proceso.
- `environment`: entorno del sistema administrado por proveedor (Arch Linux con WSL2). No se representa como binario Windows ni se elimina junto con SST.
- `system`: software que modifica Windows y requiere una instalación y confirmación especial; no se instala silenciosamente.

## Manifiesto de ejecución

Cada entrada del catálogo debe contener:

```json
{
  "format": 1,
  "id": "powershell",
  "name": "Motor PowerShell SDK para SST",
  "version": "latest",
  "category": "module",
  "portability": "isolated",
  "source": {
    "type": "github_release",
    "repository": "glitchaka/sst-blueversion",
    "asset_contains": ["SST-PowerShell-win-x64.zip"]
  },
  "install": { "type": "zip", "expose": [] },
  "execution": {
    "kind": "embedded",
    "entry": "Sst.PowerShell.Bridge.dll",
    "commands": ["psmodule", "ps", "powershell"],
    "extensions": [".ps1"],
    "abi": "sst-hostfxr/1",
    "startup": "load"
  }
}
```

`entry` es una ruta relativa interna al paquete, nunca una ruta absoluta ni una ruta con `..`. `commands` registra nombres administrados. `extensions` enruta scripts a su intérprete (sin ejecutarlos como Nwash). `abi` fija el protocolo de módulos cargables; los módulos incompatibles deben rechazarse de forma controlada, **sin impedir el inicio de SST**.

Los paquetes `process` usan `expose` para publicar ejecutables; los `runtime` registran comandos y extensiones. `environment` define `provider` y `distribution`.

## Directorios

```text
<SST_HOME>/packages/
  installed.json                # estado de instalaciones confirmado
  repositories/                 # índices
  cache/                        # descargas transitorias
  modules/powershell/           # SDK + hostfxr + ensamblado
  runtimes/python/              # intérprete y librerías
  runtimes/ruby/
  tools/ripgrep/
  tools/jq/
  environments/arch/            # metadatos SST; NO almacena la raíz de Arch WSL
  bin/                          # comandos de paquete expuestos
```

Los módulos cargables no deben añadirse al PATH general. Los intérpretes se descubren al iniciar SST y Nwash les remite los archivos por su extensión. Los paquetes CLI siguen siendo ejecutables reales y no necesitan ABI embebida.

## Contrato de ejecución

1. `instp install`: obtiene el paquete, comprueba la descarga y verifica que `execution.entry` existe dentro del directorio aislado. Los módulos embebidos exigen todos los componentes del host.
2. Activación atómica: primero se construye en staging y solo después se actualiza `installed.json`. Un fallo conserva la versión anterior.
3. Arranque: SST recorre **solo el estado instalado**. Un archivo de catálogo sin instalar no habilita comandos. Los módulos se cargan de forma controlada; los runtimes registran sus extensiones sin ejecutar procesos al iniciar.
4. Enrutamiento: la entrada y los argumentos se entregan al módulo sin concatenar sintaxis incontrolada. Se conservan `cwd`, `stdin`, `stdout`, `stderr` y el código de salida; se respeta la captura de tuberías/redirecciones de Nwash.
5. Desinstalación: elimina únicamente el directorio propio del paquete y su registro, **no** borra archivos de proyectos ni distribuciones externas del sistema.
6. Actualización: `instp upgrade` sustituye el paquete cuando hay una versión nueva, sin romper el contrato ABI. Un cambio de ABI requiere adaptador y pruebas nuevas.
7. Seguridad: los nombres y rutas del manifiesto se validan; la fuente debe ser HTTPS para descargas; usar SHA-256 si el proveedor publica un digest; nunca cargar DLL desde el directorio de trabajo; no enviar contraseñas a argumentos, historiales o registros.

## PowerShell

El módulo debe ejecutar **PowerShell real**: cmdlets, objetos, expresiones, tuberías y `.ps1` mediante el SDK `System.Management.Automation`. La ABI `sst-hostfxr/1` transporta una solicitud estructurada y devuelve resultado, error y estado. No se inicia `powershell.exe` ni `pwsh.exe`. No se ofrecen alias falsos como sustitutos del motor.

## Arch Linux

```json
{
  "format": 1,
  "id": "arch",
  "category": "environment",
  "portability": "system",
  "source": { "type": "system" },
  "install": { "type": "system", "expose": [] },
  "execution": {
    "kind": "environment",
    "entry": "wsl.exe",
    "commands": ["arch"],
    "extensions": [],
    "provider": "wsl",
    "distribution": "archlinux",
    "startup": "discover"
  }
}
```

Solo una orden **explícita** `instp install arch` podrá solicitar `wsl --install archlinux`. SST no habilitará WSL ni instalará Arch de forma automática al arrancar. `arch COMANDO [ARGS]` usará `wsl.exe --distribution archlinux --exec ...`. `instp remove arch` separa el registro de SST, **no** hace `wsl --unregister` ni destruye datos de Linux. Arch tiene ciclo de vida persistente por ser externo a SST.

## Versiones

- `schema: 2` = índice de catálogo de paquetes con adaptadores.
- `format: 1` = contrato de metadatos por paquete.
- `abi: sst-hostfxr/1` = puente de módulos .NET alojados por SST.
- Adaptadores/proveedores nuevos necesitan validación explícita y pruebas: no se ejecuta código arbitrario suministrado por un manifiesto remoto sin restricciones.
