# instp — paquetes nativos de Shell Shock Tool

`instp` es el gestor de paquetes de SST; `sigsour` es su interfaz
interactiva. Ambos utilizan `PackageService`, un catálogo validado, el
mismo registro `installed.json` y las mismas reglas de activación.

## Qué se puede instalar

Un paquete instalable en SST tiene extensión **`.pkgx`** y contrato
**SST-PACKAGE/1**. El contenedor es TAR comprimido mediante GZIP con
`manifest.json`, rutas `payload/` y SHA-256 por archivo. No es un
`.exe` ni un instalador externo.

En la actualidad el catálogo oficial ofrece el módulo PowerShell adaptado
al proceso SST. Los nombres de herramientas externas son candidatos para
implementación futura, **no paquetes instalables**. Por ejemplo, la lista
de paquetes de Arch es una referencia para desarrollar equivalentes
nativos SST; **no instala Arch Linux, WSL2 ni ejecuta `pacman`**.

Cada módulo debe declarar cómo se carga (`execution.kind`, `entry`,
`abi`, `commands`, `extensions`, `required_files` y `target`).
El archivo no implementa por sí mismo un intérprete: la funcionalidad
requiere un adaptador de SST.

## Comandos

```text
instp search [TEXTO]                    buscar módulos adaptados
instp info PAQUETE                      identidad, origen, contrato y estado
instp install PAQUETE                   instalar temporalmente
instp install PAQUETE -p                conservar entre generaciones
instp install RUTA\\MODULO.pkgx -p     instalar desde archivo local válido
instp list                              ver instalaciones activas
instp remove PAQUETE                    retirar de SST
instp upgrade [PAQUETE]                 actualizar un módulo instalado
instp update                            actualizar solo índices de paquetes
instp check sst                         consultar actualizaciones del núcleo
instp upgrade sst                       verificar y preparar actualización SST
instp sst                               estado de instalación y canal SST
instp repo list                         ver repositorios
instp repo add NOMBRE URL [PRIORIDAD]   registrar catálogo HTTPS
instp repo remove NOMBRE                retirar catálogo
instp repo update                       refrescar índices
instp cache clean                       limpiar descargas temporales
instp doctor                            diagnóstico del gestor
sigsour                                 abrir el catálogo interactivo
```

**`instp update` no actualiza el ejecutable de SST.** La actualización del
núcleo utiliza `instp upgrade sst`, descarga un ejecutable verificado y
lo aplica al cerrar la consola, sin reemplazar el archivo que está abierto.

## Directorios

```text
<SST_HOME>/
  packages/
    installed.json
    repositories/
    cache/
    win/
      modules/
        powershell/
      data/
    unix/
      modules/
      data/
```

La estructura contempla subárboles de plataforma, pero ninguna carpeta
`unix` implica instalar o ejecutar una distribución Linux. Solo se
cargan módulos que tengan contrato y estado de instalación válidos.

## Seguridad y actualización

1. Obtener el `.pkgx` desde un catálogo HTTPS aprobado o ruta local.
2. Comprobar el manifiesto, rutas, límites, SHA-256 y archivos obligatorios.
3. Descomprimir en staging, nunca directamente sobre un módulo activo.
4. Activar el directorio y registrar `installed.json` de forma transaccional.
5. En el siguiente arranque, cargar exclusivamente los módulos instalados
   cuyo adaptador es compatible.
6. Conservar la instalación anterior si falla la actualización.

No se aceptan ejecutables externos dentro de `.pkgx` ni referencias a
instaladores de sistemas operativos en el catálogo nativo. Los índices
antiguos incompatibles se omiten para no bloquear el arranque; los registros
antiguos de ejecutables externos o WSL dejan de activarse, sin destruir
archivos del usuario ni distribuciones ajenas.

**Persistencia:** `instp install PAQUETE` es temporal;
`-p`/`--persistent` mantiene el módulo entre generaciones funcionales
de SST. El sufijo de corrección `-blue.N` no cambia por sí solo la
generación funcional.

## Desarrollar adaptaciones

La lista de aplicaciones de Arch y otras fuentes externas sirve para
identificar capacidades deseadas. Para convertir una de ellas en módulo
SST se necesita código o un adaptador real que atienda Nwash y un archivo
`.pkgx` firmado/verificable según el estándar. No basta con comprimir
un ejecutable de terceros.

Generar un paquete de módulo:

```text
python scripts/pack-pkgx.py --id powershell --version 7.5.13 --target win --kind embedded --source .build/plugin-powershell --output dist/SST-PowerShell-win-x64.pkgx --exclude-dotnet-apphost --require-file Sst.PowerShell.Bridge.dll --require-file hostfxr.dll --require-file Sst.PowerShell.Bridge.runtimeconfig.json
```

Para el contrato exacto, ver `packages/SST-PACKAGE-v1.md`.
