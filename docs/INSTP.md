# instp — gestor portable de paquetes SST

`instp` significa **Install Package**. Su objetivo no es reemplazar el gestor de paquetes de Windows, sino permitir que Shell Shock Tool transporte runtimes y herramientas sin instalarlos globalmente en el equipo anfitrión.

## Principios

1. **SST primero.** Los paquetes se guardan bajo la carpeta portable de SST.
2. **No tocar el PATH global.** SST modifica solamente el entorno de su propio proceso antes de crear Nwash.
3. **Repositorios de metadatos.** El repositorio SST describe paquetes; los binarios pueden seguir alojados en GitHub Releases, Microsoft, RubyInstaller u otros upstream oficiales.
4. **Sin scripts arbitrarios.** Un manifiesto puede seleccionar un backend soportado, pero no introducir PowerShell, cmd o Bash libre para ejecutarlo durante una instalación.
5. **Procedencia visible.** `instp info` y el resultado de `instp install` muestran de dónde se obtiene el paquete.
6. **Verificación.** Cuando el upstream entrega un SHA-256, SST lo verifica antes de extraer. Los repositorios futuros deben añadir firma del índice.
7. **SYSTEM no es portable.** Drivers, servicios e instaladores que modifican Windows se catalogan como `system` y no se instalan silenciosamente.

## Capas

```text
Nwash
  └─ builtin instp
       └─ application::packages::PackageService
            ├─ catálogo/repositories
            ├─ resolución de versiones y fuentes
            ├─ descarga
            ├─ SHA-256
            ├─ cache
            ├─ instalación/extracción
            ├─ estado instalado
            └─ entorno portable SST
```

Nwash no conoce GitHub Releases, ZIP, Ruby ni la estructura de los repositorios. El builtin traduce comandos y delega en `PackageService`.

## Layout portable

```text
SST/
├─ sst.exe
├─ packages/
│  ├─ bin/
│  ├─ cache/
│  ├─ repositories/
│  ├─ runtimes/
│  ├─ tools/
│  ├─ ecosystems/
│  ├─ installed.json
│  └─ repos.json
├─ config/
├─ data/
└─ bg/
```

Las rutas guardadas en `installed.json` son relativas a la raíz de SST para que la carpeta pueda moverse entre unidades/equipos.

## Repositorio inicial

El catálogo compilado está en:

```text
packages/index.json
```

SST siempre puede usarlo como fallback. El repositorio remoto por defecto apunta al mismo archivo en `main` mediante `raw.githubusercontent.com`. Por eso una instalación ya existente puede aprender paquetes nuevos con:

```bash
instp update
instp upgrade [PAQUETE]
```

sin recompilar `sst.exe`.

Los repositorios cacheados tienen prioridad sobre el catálogo embebido. Cada ID aparece una sola vez y gana el repositorio habilitado con mayor prioridad.

## Esquema de paquete

Ejemplo conceptual:

```json
{
  "id": "ripgrep",
  "name": "ripgrep",
  "version": "latest",
  "description": "Fast recursive text search",
  "category": "cli",
  "portability": "pure",
  "source": {
    "type": "github_release",
    "repository": "BurntSushi/ripgrep",
    "asset_contains": ["x86_64-pc-windows-msvc", ".zip"],
    "asset_excludes": []
  },
  "install": {
    "type": "zip",
    "expose": ["rg.exe"]
  }
}
```

### Fuentes admitidas en la primera implementación

- `github_release`: consulta `releases/latest`, selecciona un asset mediante filtros y utiliza el digest SHA-256 de GitHub cuando esté presente.
- `url`: URL HTTPS directa del proveedor.

Backends previstos: PyPI/uv, RubyGems, npm, crates.io/Cargo, Packagist/Composer y repositorios offline.

### Instaladores admitidos en la primera implementación

- `single`: un solo ejecutable.
- `zip`: extracción interna con protección contra zip-slip.
- `archive`: extracción con `tar.exe` de System32.
- `system`: declarado pero rechazado por el instalador portable actual.

## Portabilidad

| Clase | Significado |
| --- | --- |
| `pure` | binario/archivo autocontenido, sin modificación del sistema |
| `isolated` | runtime/herramienta con árbol propio dentro de SST |
| `system` | necesita MSI/EXE, servicio, driver o cambios permanentes en Windows |

Los tipos `system` quedan en el catálogo para búsqueda/planificación, pero esta fase no los instala automáticamente.

## Separación de interfaces y retención

- `instp` es exclusivamente CLI, comparable al motor de pacman. Sin argumentos muestra su ayuda y `instp PAQUETE` solo consulta el estado.
- `sigsour` es el comando independiente de la TUI de paquetes de SST y utiliza el mismo `PackageService` que `instp`. El nombre fue elegido explícitamente para la interfaz visual.
- `instp install PAQUETE` instala temporalmente. `-p` o `--persistent` indica explícitamente que debe conservarse al actualizar la versión funcional.
- La edición `blue` identifica SST TI. El número `blue.N` identifica revisiones de corrección; no debe interpretarse como otra edición ni como frontera de caducidad.
- La versión funcional se deriva de la versión oficial `Cargo.toml` (por ejemplo `0.1.9-blue.27` pertenece a la generación `0.1.9`). Cuando cambia esa generación, se limpian los paquetes temporales y sus archivos, manteniendo los persistentes. La migración inicial respeta los paquetes existentes hasta una actualización futura.
- `instp update` refresca índices. `instp upgrade [PAQUETE]` busca las versiones publicadas actualmente y actualiza las herramientas ya instaladas, conservando la política temporal/persistente.

## Comandos

```text
instp [PAQUETE]
instp search [TEXTO]
instp info PAQUETE
instp install PAQUETE [-p|--persistent]
instp remove PAQUETE
instp list
instp update
instp which COMANDO

instp repo list
instp repo add NOMBRE URL [PRIORIDAD]
instp repo remove NOMBRE
instp repo update

instp cache clean
instp doctor
```

## Entorno

Antes de crear `NativeShellEngine`, SST reconstruye el PATH de su proceso usando `installed.json`. `ShellEnvironment::new()` hereda ese entorno y los procesos hijos reciben después el mismo PATH mediante Nwash.

Nunca se utiliza `setx`, el registro de `Environment`, el PATH del usuario ni el PATH del sistema.

Para Ruby, SST reserva además:

```text
packages/ecosystems/ruby
```

como `GEM_HOME`/`GEM_PATH`.

## Seguridad

- fuentes remotas del catálogo: HTTPS obligatorio;
- `curl.exe`: System32, no resolución por el directorio actual;
- `tar.exe`: System32;
- ZIP: nombres encerrados (`enclosed_name`) para impedir extracción fuera del destino;
- identificadores restringidos a caracteres seguros;
- hashes SHA-256 comprobados cuando el upstream los expone;
- no hay comandos de instalación arbitrarios en el índice;
- paquetes `system` rechazados en esta fase.

### Pendiente antes de considerar estable el formato de repositorio

- firma criptográfica del índice y gestión de claves confiables;
- lockfile con versión/digest exactos;
- rollback transaccional de instalaciones fallidas;
- dependencias declarativas;
- mirrors/offline bundles;
- adapters de ecosistemas (`py:`, `gem:`, `npm:`, `cargo:`);
- política explícita para SYSTEM/DRIVER y elevación mediante el broker de SST.

## Bundles/offline — siguiente etapa

El mismo formato debe poder materializarse posteriormente en una carpeta o unidad extraíble:

```text
repo/
├─ index.json
└─ objects/
```

`instp bundle fetch admision E:\RepoAdmision` deberá descargar los objetos y hashes necesarios desde una máquina con Internet. La red cerrada podrá consumir el mirror sin cambiar los manifiestos ni el resolver.
