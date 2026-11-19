# SST-PACKAGE/1 — formato de distribución e instalación de Shell Shock Tool

## Contenedor propio

La extensión oficial es **`.sstpkg`**. El contenido es un TAR comprimido con
GZIP (`.tar.gz`), no un ejecutable ni un ZIP renombrado. `instp` puede
instalar paquetes registrados desde un repositorio SST o desde un archivo local:

```text
instp install powershell [-p|--persistent]
instp install C:\\Descargas\\powershell.sstpkg [-p|--persistent]
instp upgrade powershell
instp remove powershell
instp upgrade sst             # Actualiza la propia consola por su canal blue
```

El catálogo usa `schema: 2` y cada entrada `format: 1`,
`install.type: "sstpkg"` y un adaptador `execution`. Los archivos de
proveedores originales (`.zip`, `.7z`, `.tar.gz`, `.exe`) pueden describirse
para consulta, pero **no son paquetes instalables en SST** hasta que se
conviertan en integraciones reales. Está prohibido instalar ejecutables
adicionales: los comandos deben pasar por Nwash y módulos de SST.

### Estructura del archivo

```text
powershell-7.5.13-win.sstpkg  (TAR + GZIP)
├── manifest.json
└── payload/
    ├── win/
    │   ├── modules/
    │   │   └── powershell/
    │   │       ├── Sst.PowerShell.Bridge.dll
    │   │       ├── Sst.PowerShell.Bridge.runtimeconfig.json
    │   │       └── ... DLL y dependencias del SDK
    │   ├── runtimes/
    │   ├── environments/
    │   └── data/
    └── unix/
        ├── modules/
        ├── runtimes/
        ├── environments/
        │   └── arch/
        │       ├── etc/
        │       ├── usr/
        │       └── ...
        └── data/
```

Un paquete puede contener simultáneamente árboles `win` y `unix`; se
verifican los hashes de **todos** los archivos, pero solo se despliega el árbol
de la plataforma declarada. Sus carpetas anidadas se conservan. Por ejemplo,
`payload/unix/environments/arch/etc/foo.conf` se copia a
`<SST_HOME>/packages/unix/environments/arch/etc/foo.conf`.

El `manifest.json` debe ser el primer elemento del TAR y tiene esta forma:

```json
{
  "format": 1,
  "id": "powershell",
  "version": "7.5.13",
  "kind": "embedded",
  "files": [
    {
      "path": "payload/win/modules/powershell/Sst.PowerShell.Bridge.dll",
      "sha256": "HASH_SHA256_DE_64_CARACTERES_HEXADECIMALES"
    }
  ]
}
```

El ejemplo es esquemático: un paquete real enumera **todos** sus archivos con
el SHA-256 de cada uno, y cada digest contiene 64 caracteres hexadecimales.
No se admiten archivos no declarados, rutas absolutas, recorridos con `..`,
enlaces, dispositivos ni sufijos de ejecutables `.exe`, `.com`, `.bat` o
`.cmd`. Se limita el tamaño individual, total y la cantidad de entradas.

### Directorio instalado

```text
<SST_HOME>/packages/
├── installed.json                 # Registro transaccional de instalaciones
├── repositories/                  # Catálogos
├── cache/                         # Descargas temporales
├── win/
│   ├── modules/powershell/        # DLL del motor PowerShell, sin .exe
│   ├── runtimes/
│   ├── environments/
│   └── data/
└── unix/
    ├── modules/
    ├── runtimes/
    ├── environments/arch/         # Metadatos y paquetes Unix propios de SST
    └── data/
```

La distribución Arch WSL2 real está administrada por Windows/WSL y **no**
corresponde al directorio de metadatos de SST. Instalar archivos Unix en este
árbol de SST no implica automáticamente instalar paquetes en la raíz de WSL:
la instalación de software dentro de Arch exige un adaptador Unix/WSL
autorizado. `instp install arch` conserva su flujo de provisión explícita
mediante WSL2; `instp remove arch` no elimina la distribución del usuario.

## Adaptadores y ciclo de vida

- `embedded`: bibliotecas cargadas dentro de SST mediante una ABI declarada.
  Actualmente PowerShell usa `sst-hostfxr/1` con el SDK .NET y ejecuta `.ps1`,
  cmdlets y tuberías sin abrir `powershell.exe` ni `pwsh.exe`.
- `runtime`: contrato de intérpretes. No se autoriza entregar un intérprete
  `.exe` como si fuera un plugin; necesita un adaptador cargable real.
- `environment`: árboles Unix/entornos SST o provisión explícita mediante
  proveedor del sistema como WSL2.
- `data`: archivos sin código ejecutable que preservan sus subdirectorios.
- `process`/CLI externo: **fuera de la distribución interna por ahora**.
  Herramientas de terceros se mantienen en el catálogo solo para identificar
  las integraciones pendientes; no se instalarán sus `.exe`.

La instalación debe pasar siempre por staging, verificar el manifiesto y los
archivos obligatorios del adaptador, y activar atómicamente el nuevo directorio
y `installed.json`. Un error conserva la instalación anterior. Los paquetes
persistentes `-p` sobreviven cambios de generación; los temporales siguen
las reglas de `instp`.

La carga es por contrato: `execution.kind`, `execution.target` (`win` o
`unix`), `execution.entry`, `execution.required_files`, `execution.commands`,
`execution.extensions` y, cuando corresponde, `execution.abi`.
No se ejecutan DLL ni scripts porque aparezcan en un directorio: deben estar
registrados en el estado instalado de SST.

Para generar un paquete sin crear otro ejecutable:

```text
python scripts/pack-sstpkg.py --id powershell --version 7.5.13 --target win --kind embedded --source target/plugin-powershell --output dist/SST-PowerShell-win-x64.sstpkg --exclude-dotnet-apphost
```

**Límite de implementación:** este formato y el despliegue por plataforma son
genéricos, pero las ABIs de ejecución aún requieren adaptadores compatibles.
Un contenedor válido no convierte por sí solo un programa Windows externo en
un módulo SST ni instala binarios de Linux dentro de la distribución WSL.
