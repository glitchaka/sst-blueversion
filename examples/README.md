# SST / Nwash — ejemplos de scripting

Estos ejemplos están pensados como **demostraciones reales de la consola**, no como snippets mínimos.

Se pueden ejecutar desde SST con:

```bash
examples/01-language-tour.sh
```

o explícitamente:

```bash
sst.exe examples/01-language-tour.sh
```

## Contenido

| Archivo | Demuestra |
|---|---|
| `01-language-tour.sh` | funciones, arrays, arrays asociativos, condicionales, `case`, aritmética, here-docs y expansiones |
| `02-windows-operator-report.sh` | composición de builtins SST/Nwash para generar un informe operativo de Windows |
| `03-network-discovery.sh` | descubrimiento, inventario, snapshots de red y process substitution |
| `04-jobs-and-coproc.sh` | jobs, `$!`, `jobs`, `wait -n`, coprocesos y descriptores |
| `05-pipelines-and-text.sh` | pipelines, redirecciones y utilidades Unix integradas |
| `06-security-triage.sh` | triage local, persistencia, servicios, firmas y fuentes intel |
| `07-operator-console.sh` | menú interactivo con `select`, funciones y composición de herramientas SST |
| `08-config-and-backgrounds.sh` | configuración portable y administración de fondos `config bg` |
| `09-advanced-bash.sh` | `getopts`, namerefs, arrays dispersos, `mapfile`, `printf -v`, parámetros y stack de funciones |

Los scripts son deliberadamente **no destructivos**: consultan estado, generan reportes y crean archivos temporales, pero no reinician servicios, borran tareas ni modifican ACL/Registro.

Algunos collectors pueden requerir privilegios elevados para entregar todos los datos. En ese caso el ejemplo deja que SST muestre la limitación en vez de ocultarla.
