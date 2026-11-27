# SST / Catálogo de elementos gráficos

El catálogo existe dentro de SST (`demo graficos`), no en ilustraciones ni informes ficticios. Se prueban componentes aislados a escala de interfaz. La referencia brutalista y retrofuturista se traduce a recortes angulares, vacíos arquitectónicos, señales, escalas y jerarquía de texto. El logo original permanece intacto.

## 1. Estructura y recortes

- Cabecera asimétrica
- Divisor escalonado
- Marco recortado
- Agrupador de campos
- Pestaña de metadatos
- Riel y folio

## 2. Foco y controles

- Marcas de registro
- Foco de teclado
- Fila activa
- Selector angular
- Acciones
- Expansión

## 3. Instrumentación

- Indicador radial / 20
- Anillo segmentado / 8
- Escala lineal
- Umbral y desviación
- Lectura proporcional
- Escala de actividad

## 4. Transferencia y señal

- Transferencia angular
- Progreso segmentado
- Espera indeterminada
- Continuidad y pérdidas
- Ruta / bifurcación
- Trama / transferencia

## 5. Estados y evidencia

- Correcto
- Advertencia
- Fallo
- Sin datos
- Evidencia
- Estado dinámico

## 6. Tipografía y navegación

- Título compacto
- Folio numérico
- Microtipografía
- Entrada y cursor
- Sugerencias
- Jerarquía de dato

La demo conserva estados simulados; la terminal habitual emplea las primitivas compartidas con datos reales. Las cabeceras, agrupadores, divisores y medidores se calculan desde las filas visibles, incluidas salidas externas en directo y scrollback. La selección y las aplicaciones de pantalla completa conservan su comportamiento.

El catálogo hereda el esquema activo de config palette (gruvbox, sunset o golden). Los colores de los componentes se derivan de los roles de texto del formatter; los fondos del usuario y el logotipo no cambian. El agrupador de campos sustituye al pórtico decorativo.

### Esquemas disponibles (config palette NOMBRE)

- `gruvbox`: original
- `sunset`: #3D4D65 #032451 #CE2F25 #F94C1F #FE8005 #FBBA76
- `golden`: #E3D473 #D72E2E #0E5047 #22153C #000F08
- `robot`: #88B8FF #213878 #C1911B #B3441A #521E1A
- `cyberpunk`: #FD8326 #F59192 #F4545E #D32729 #6D2A4F #25091C
- `stellar`: #142030 #E9D8C8 #732553 #FF5C8D #1E3442 #85A3B2
- `aqua`: #78161E #EF3E18 #94EEE3 #33C6BA #2E5C58
- `industrial`: #222A2A #4FBBBC #EC4624 #F39120 #E2DAC2
- `oxidize`: #1D0F28 #E10109 #FF1F0D #FF8C01 #0C7473
- `starfield`: #D5DAD7 #D3A54F #D35F3F #B72B3C #334A73 #1F2C41
- `ronin-sunset`: #FDB563 #E67090 #B96682 #685A7D #3B5D80
- `palm-leaf`: #364023 #6A823E #9C9F69 #C7A39B #E6B1C4 #EFD4DD
- `brown-coffee`: #461D3A #502A50 #7E2A53 #BA71A2 #ECD0EC
- `blaze-orange`: #F06B04 #FB8728 #F5ECD2 #269FC7 #1570AC #A10815
- `japanese-violet`: #502A50 #7E2A53 #BA71A2 #E6B1C4 #EFD4DD
- `curious-blue`: #269FC7 #1570AC #364023 #6A823E #C7A39B
- `classic-rose`: #E6B1C4 #EFD4DD #BA71A2 #7E2A53 #A10815
- `mondongo`: #7B7A7A #FFFF00 #00FFF7 #FF8B00 #8511FF #008F39

Oxidize y ronin-sunset: colores muestreados de las imágenes JPEG, no códigos publicados. Las otras referencias conservan los HEX impresos o los RGB del archivo GPL. Los tonos oscuros pueden aclararse para lectura y se conservan los colores semánticos SST cuando falta un tono de estado adecuado. No cambia el fondo ni el logotipo.

Dentro de `demo graficos`, pulsa P para la paleta siguiente y O para la anterior. El nombre del esquema aparece en la cabecera. Las 18 paletas se recorren circularmente; el cambio conserva la familia, foco y animación. Al salir se conserva la configuración original de `config palette`. Para fijar un esquema de forma permanente, usa `config palette NOMBRE` fuera de la demo.

La identidad gráfica se aplica ahora también a la terminal habitual: los trazados compartidos del catálogo se usan en marcos y transferencias; las cabeceras, tablas, selección, cursor e indicadores de métricas utilizan los roles de la paleta activa. La salida de máquina y los datos de los comandos se conservan. Los 36 elementos tienen que estar vinculados a una función de la terminal habitual; la demo se conserva exclusivamente como catálogo de referencia.

## Integración de la terminal habitual

La revisión .9 no es una entrega aprobada. Las primitivas compartidas de instrumentación y transferencia utilizan CPU, RAM y bytes reales. Los marcos de monitor mantienen esquinas de tamaño fijo. Los estados explícitos y la entrada editable reciben geometría en el viewport. El bloqueo de la revisión .9 se conserva; la revisión .10 usa un tag distinto y la integración corregida. La evidencia visual se conserva con los resultados de Windows.

La selección nativa de Tab usa los candidatos que entrega el motor, con aceptar/cancelar, expansión y navegación. Las escalas reciben los porcentajes del formatter; la continuidad utiliza respuestas y pérdidas observadas y las bifurcaciones representan destinos que comparten next hop e interfaz en la tabla visible. Los folios de monitores corresponden a su índice real. La revisión .9 permanece retirada. La compilación de la revisión fuente 6112244972fdd9e7fc2d78e192d7c37481b8e54a, sus regresiones y las capturas de sys, ping, sys info, rutas, Tab y aqua fueron completadas en Windows; no equivalen a una inspección visual exhaustiva de las 36 muestras en todos los contextos.

La validación de Windows incluye capturas de la ventana del ejecutable real, no composiciones generadas ni imágenes sustitutas. Las capturas son evidencia de revisión, no una aprobación automática de la fidelidad artística.

**Límite de integración:** la isla superior queda fuera del catálogo. Los indicadores radiales y anillos pertenecen a las métricas de los informes de la terminal y al monitor de procesos, no a la barra de ventana. El informe `sys info` expone CPU y memoria medidas, sin sustituir los campos existentes. El fondo y el logo no se modifican.

**Sys top / 04 y 12.** El elemento 12 abre o cierra el detalle del proceso seleccionado mediante Enter (o clic en su cabecera). El agrupador 04 organiza PID, PPID, estado, CPU, RAM, tiempo y nombre dentro de filas propias del monitor. La tabla reduce su altura mientras el detalle está abierto. En ventanas insuficientemente altas el detalle queda contraído. No hay acciones destructivas. El prompt conserva exclusivamente su marcador y cursor; las marcas de registro no se emplean allí.

**Contrato de componentes.** `ExpandableFields` es un componente reutilizable del formatter. La herramienta proporciona título, campos, valores y estado de expansión; el formatter calcula filas, alineación, marcador y roles de superficie. El renderer interpreta esos roles sin comprobar el nombre del comando ni textos como PID o DETALLE. El estado de selección y la acción de abrir/cerrar pertenecen al controlador de la herramienta, no al dibujo.

## Revisión consolidada de integración

El formatter central es responsable de la composición tanto de informes como de monitores y salidas estructuradas. La GUI proyecta las marcas semánticas de `native_layout.rs` sin seleccionar componentes por nombre de comando. La cabecera es compacta y no ocupa el ancho completo. Los agrupadores reservan un margen real; la continuidad se sitúa en el resumen de observaciones, con etiqueta. Se eliminan folios automáticos sin significado y los adornos del prompt. La selección no elimina la estructura del informe. Los datos de arranque junto al arte ANSI usan roles de texto del formatter, conservando los bytes del arte. No se declara completa la integración de los 36 elementos: los indicadores de tamaño propio y los demás contextos requieren revisión antes de publicar.

## Composición común de métricas y monitores

Los campos medidos y acotados tienen una región de tres filas asignada por el formatter (`metric_grid`), con etiqueta y valor exacto. CPU usa el radial, utilización el anillo y pérdida la escala lineal. No se calculan porcentajes a partir de magnitudes sin denominador. El renderer interpreta tokens semánticos reservados y no nombres de comandos. `monitor_summary` adapta la presentación al alto y ancho disponibles; los paneles genéricos no se recuadran automáticamente ni reciben folios. Los registros numerados reciben jerarquía tipográfica local. La selección no elimina la región de las métricas. No se modifican la isla, el fondo, el logotipo ni las paletas originales.

Los monitores utilizan `monitor_table` para medir columnas, alinear valores y seleccionar roles; las aplicaciones entregan registros y acciones. `LiveScreen` aplica el mismo formatter a las secciones y limita la presentación al espacio disponible. La continuidad se representa a partir de los conteos explícitos de enviados y recibidos, no de las filas visibles ni de un orden supuesto. Las rutas solo generan una bifurcación cuando existe una relación real de destinos que comparten gateway e interfaz, con encabezado de grupo. Las marcas de registro no se añaden al final de una línea de evidencia.

La lista consolidada y la matriz de los 36 componentes están en [EDITORIAL_ACCEPTANCE.md](EDITORIAL_ACCEPTANCE.md). Es un control de aceptación de la revisión en curso, no una declaración de entrega.
