# SST-PACKAGE/1 — contrato nativo de paquetes de Shell Shock Tool

## Propósito

**SST instala integraciones adaptadas a su propio motor, no programas independientes.**
El contenedor oficial lleva extensión **`.pkgx`** y usa TAR comprimido con
GZIP. No es un ejecutable, un instalador de terceros ni un archivo ZIP renombrado.

`instp` y `sigsour` consumen el **mismo catálogo validado**. Solo pueden
publicarse paquetes con `install.type: "pkgx"` y un adaptador de ejecución
permitido. Las referencias a programas externos **no forman parte** del
catálogo instalable.

Ejemplos:

```text
instp search
instp info powershell
instp install powershell [-p|--persistent]
instp install C:\\Descargas\\powershell.pkgx [-p|--persistent]
instp upgrade powershell
instp remove powershell
instp upgrade sst              # Actualiza el núcleo, no instala un .pkgx
```

## Estructura de contenedor

```text
SST-PowerShell-win-x64.pkgx       (TAR + GZIP)
├── manifest.json                 # Primera entrada del TAR
└── payload/
    └── win/
        └── modules/
            └── powershell/
                ├── Sst.PowerShell.Bridge.dll
                ├── Sst.PowerShell.Bridge.runtimeconfig.json
                ├── hostfxr.dll
                └── ... dependencias
```

El formato admite `payload/win/` y `payload/unix/` con directorios
`modules`, `runtimes`, `environments` y `data` para **cargas nativas
compatibles con SST**. Que exista un árbol Unix en el formato no implica
instalar Linux, invocar WSL o ejecutar herramientas de Arch. Solo se activa el
árbol de plataforma declarado y se verifican los hashes de ambos si coexisten.

Ejemplo esquemático del `manifest.json`:

```json
{
  "format": 1,
  "id": "powershell",
  "version": "7.5.13",
  "kind": "embedded",
  "files": [
    {
      "path": "payload/win/modules/powershell/Sst.PowerShell.Bridge.dll",
      "sha256": "SHA256_REAL_DE_64_DIGITOS_HEXADECIMALES"
    }
  ]
}
```

En un paquete real cada archivo se enumera con su SHA-256 verdadero. El
instalador rechaza entradas fuera del manifiesto, rutas de escape, enlaces,
nodos especiales, archivos duplicados y ejecutables independientes
(`.exe`, `.com`, `.bat`, `.cmd`). También limita el número y tamaño de
archivos y el volumen total descomprimido.

El catálogo usa `schema: 2` y los paquetes `format: 1`.
El contrato de ejecución incluye `execution.kind`, `target`, `entry`,
`required_files`, `commands`, `extensions` y, donde corresponda,
`abi`. Las rutas y archivos obligatorios se validan antes de activar el
paquete. `installed.json` es el registro de estado y el staging se activa
de forma transaccional, conservando la versión anterior ante errores.

## Adaptaciones, no instalaciones externas

- **PowerShell** es un módulo específico de SST, alojado mediante
  `System.Management.Automation` y `sst-hostfxr/1`. Su módulo `.pkgx`
  se integra al proceso SST y atiende comandos y scripts `.ps1`; no abre
  `powershell.exe` ni `pwsh.exe`.
- **Paquetes de Arch**: el listado de paquetes de Arch es **únicamente una
  fuente de funcionalidades que se estudiarán y adaptarán a SST**. No existe
  `instp install arch`, adaptador WSL2 ni comando `arch` distribuido por SST.
  Cada adaptación futura requerirá su módulo o adaptador nativo y su propio
  `.pkgx`.
- **Herramientas de proveedores**: archivos `.exe`, `.zip`, `.deb`,
  `.rpm` u otros formatos externos no se consideran paquetes SST y no
  aparecen como instalables en `sigsour`.

Los adaptadores `runtime` y `process` que impliquen ejecutar programas
externos no están habilitados por el estándar instalable actual. La presencia
de un nombre de tecnología en una lista de investigación no concede permiso
para instalarla ni ejecutarla.

## Generación

```text
python scripts/pack-pkgx.py --id powershell --version 7.5.13 --target win --kind embedded --source .build/plugin-powershell --output dist/SST-PowerShell-win-x64.pkgx --exclude-dotnet-apphost
```

**Alcance implementado:** generación de un contenedor, validación de
manifiestos y SHA-256, instalación local o desde catálogo, staging,
registro y activación de módulos con un adaptador compatible. El formato
por sí solo no implementa un intérprete nuevo ni adapta automáticamente
herramientas de terceros.
