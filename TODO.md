# SST TODO

## Hoja de ruta posterior a 0.1.9-blue.11 (planificada, NO incluida en la release correctiva)

### Actualizaciones
- [ ] Después de aplicar una actualización y relanzar SST, mostrar temporalmente las novedades de la versión aplicada antes de continuar al prompt normal.
- [ ] Las novedades deben provenir de los cambios publicados con la versión, no quedar codificadas de forma permanente en el ejecutable.

### `guide`: contenido offline por apartados
- [ ] Mantener el repositorio remoto como fuente de verdad del contenido de `guide`.
- [ ] Permitir descargar contenido para uso offline por apartado, no como un paquete monolítico.
- [ ] Sin conexión, mostrar únicamente los apartados que el usuario haya descargado previamente; si no hay ninguno descargado, `guide` debe quedar vacío.
- [ ] Permitir actualizar y eliminar la copia offline de cada apartado de forma independiente.
- [ ] Mostrar `Arch / Portabilidad` como apartado propio de primer nivel dentro de `guide`, pero no tratarlo como documentación ni como un manual normal.

### `Arch / Portabilidad`: catálogo de revisión, no tutorial
- [ ] Al abrir `Arch / Portabilidad`, entrar a una vista interactiva de revisión de candidatos; no abrir un `.md`, tutorial, manual o texto explicativo.
- [ ] Mantener su índice/datos separados del catálogo de comandos de `guide`, de sus búsquedas y de sus manuales.
- [ ] Presentar una lista navegable de paquetes/proyectos candidatos con información útil para decidir qué portar, evitando bloques de explicación general.
- [ ] La lista debe permitir buscar y filtrar candidatos y mostrar como mínimo: nombre/proyecto, propósito breve, upstream, lenguaje, licencia, dependencias/plataforma relevantes y estado de revisión.
- [ ] Al abrir un candidato, mostrar una ficha técnica de decisión: componentes útiles, dependencias Linux/POSIX que habría que sustituir, capacidades ya cubiertas por SST, partes que conviene portar, dificultad/riesgo y destino previsto dentro de SST.
- [ ] Estados de decisión: `PENDIENTE`, `APROBADO`, `DESCARTADO`, `IMPLEMENTADO`.
- [ ] No usar como criterio principal si existe una versión Windows ya hecha; la revisión parte del upstream original y de qué código/funcionalidad conviene portar a SST.
- [ ] El resultado preferido de un port aprobado es funcionalidad nativa SST o módulo nativo SST, no un `.exe` externo por defecto.

### Módulos nativos SST
- [ ] Para herramientas aprobadas que deban distribuirse como extensiones, usar módulos nativos cargables por SST, no ejecutables externos como diseño por defecto.
- [ ] Diseñar un ABI estable para módulos nativos y una API SST para argumentos, stdin/stdout/stderr, entorno, filesystem, red, interrupciones y demás servicios compartidos.
- [ ] Evitar depender del ABI de Rust entre SST y los módulos; usar una frontera estable compatible con código nativo.
- [ ] Resolver dependencias primero contra capacidades que SST ya pueda proporcionar; descargar módulos adicionales solo cuando una capacidad realmente falte.
- [ ] Distinguir dependencias directas, transitivas y opcionales.

### `instp`: un solo motor de gestión de paquetes
- [ ] `instp` debe ser el único motor de gestión de paquetes de SST. No crear un segundo gestor independiente para la interfaz TUI.
- [ ] Mantener `instp` como comando/CLI y motor de paquetes; la TUI de catálogo/gestión debe tener un nombre propio diferente. Nombre elegido e implementado: `sigsour`.
- [ ] La TUI con nombre propio debe usar exactamente el mismo motor `instp`: mismo catálogo, resolver de dependencias, estado instalado, verificación, transacciones y operaciones. Solo cambia la interfaz.
- [ ] `instp` sin argumentos no debe abrir la TUI por defecto; seguirá siendo la interfaz CLI de `instp`.
- [ ] `instp <paquete>` sin verbo de instalación/actualización debe ser una consulta de estado, nunca una instalación implícita.
- [ ] La consulta debe distinguir como mínimo:
  - paquete instalado: mostrar versión instalada y, si se conoce, versión disponible;
  - paquete disponible pero no instalado;
  - paquete no encontrado en el catálogo.
- [ ] `instp update` debe actualizar el catálogo/índices de paquetes.
- [ ] `instp upgrade` debe actualizar paquetes instalados según las reglas definidas por `instp`.
- [ ] `instp install <paquete>` debe instalar el paquete y resolver sus dependencias/capacidades necesarias.
- [ ] La ruta de actualización debe actualizar únicamente lo solicitado o lo que corresponda según el subcomando, sin convertir una consulta simple en una transacción.

### Catálogo `instp`
- [ ] Catálogo remoto con caché local para consulta offline.
- [ ] Mostrar por paquete: nombre, descripción, versión, requisitos de SST/Windows, tamaño, comandos que proporciona, licencia, fuente/upstream, estado instalado/disponible y dependencias.
- [ ] Mostrar claramente qué dependencias ya satisface SST, cuáles están instaladas como módulos compartidos y cuáles deberán descargarse.
- [ ] El proceso de instalación debe mostrar progreso compacto por etapas, paquete actual, total, bytes y velocidad.
