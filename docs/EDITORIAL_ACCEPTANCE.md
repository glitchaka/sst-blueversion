# Control de aceptación — revisión editorial SST Blue

**Alcance:** todos los reclamos acumulados sobre .10 y .11, incluidas las capturas y vídeos de la sesión. No publicar una revisión parcial. La isla superior, el logo, el fondo y los 18 esquemas originales permanecen fuera de las modificaciones de diseño. No ejecutar pruebas adicionales sin autorización. El catálogo es un sistema de presentación, no una cuota de adornos.

## Reclamos acumulados

| ID | Criterio verificable |
|---|---|
| 01 | Ninguna decisión gráfica por nombre de comando; formatter común. |
| 02 | Monitores de estructura equivalente comparten tabla, métricas y selección. |
| 03 | Arranque legible: metadatos jerárquicos junto al logo sin alterarlo. |
| 04 | Sin recuadro junto a $; solo marcador y cursor. |
| 05 | Marcas de registro únicamente para selección o inspección real. |
| 06 | Continuidad asociada a conteos reales; etiqueta y pérdidas explícitas. |
| 07 | Cabeceras según jerarquía documental, no repetición del comando. |
| 08 | Indicadores cuantitativos en regiones propias y proporcionales. |
| 09 | Agrupar campos relacionados mediante el formatter. |
| 10 | Expansión solo con detalle real y acción funcional. |
| 11 | Revisar los 36 componentes según su función. |
| 12 | Datos importantes visibles de un vistazo; tipografía y composición. |
| 13 | Los 18 esquemas modifican todos los roles visuales sin alterar sus fuentes. |
| 14 | README, guide e identidad gráfica coherentes. |
| 15 | Sin cabeceras gigantes de ancho completo. |
| 16 | Sin diagonales repetidas como ornamento de subtítulos. |
| 17 | Sin marcos indiscriminados que ocupen toda la ventana. |
| 18 | Folios solo cuando existe numeración informativa. |
| 19 | Sin indicadores diminutos añadidos al espacio sobrante. |
| 20 | Aprovechamiento del ancho y reflujo según la ventana. |
| 21 | Separación de etiquetas, valores, estados y explicaciones. |
| 22 | Coherencia de net, notsh, sys y cualquier salida equivalente. |
| 23 | Salidas nativas Windows por el formatter interactivo, datos máquina intactos. |
| 24 | Ayudas, inventarios y diagnósticos con jerarquía consistente. |
| 25 | Eliminar línea inferior creciente del prompt. |
| 26 | Eliminar esquinas de registro del prompt. |
| 27 | Mantener comportamiento de edición, cursor y teclado. |
| 28 | Seleccionar texto no debe recomponer ni ocultar la información. |
| 29 | Mediciones, agrupaciones y advertencias identificables por su función. |
| 30 | Eliminar adornos y cabeceras que no aporten significado. |
| 31 | Trasladar la composición del catálogo al formatter, no decorar comandos. |
| 32 | Eliminar comentarios de desarrollo y justificaciones en salidas de usuario. |
| 33 | Ayudas limitadas a función, sintaxis, opciones, ejemplos y advertencias operativas. |
| 34 | Entender la información al mirarla: separar resultados principales, datos secundarios y evidencia, sin sacrificar datos. |
| 35 | Ampliar `sys info` como inventario integral del equipo: conservar las métricas existentes, incorporar información de hardware y sistema operativo y las cuentas locales disponibles en `sys users`, diferenciándolas del usuario de sesión actual. Obtener la información en una sola ejecución, con jerarquía y agrupación mediante el formatter común; distinguir datos no disponibles y conservar `sys users` para consultas específicas. |
| 36 | Resolver la regresión en la ejecución elevada de `sudo`, observada con `sudo hostname` y `sudo domain` en blue.11: recuperar stdout, stderr y estado real en la misma consola, incluyendo aviso de reinicio cuando proceda, cancelación y errores. No confundir elevación local con autenticación de dominio; conservar sintaxis y comportamiento de los comandos existentes. Corregido en fuente el escapado de rutas Windows y la pérdida silenciosa de resultados (ec91e53); pendiente validación autorizada de extremo a extremo y diagnóstico del error 1326 sin atribuirlo automáticamente a AD. |
| 37 | Restaurar `guide` y `guide TEMA` en blue.11: el alias `keygen` de `sys` colisionaba con la ficha independiente e invalidaba el catálogo entero. Corrección de fuente: prioridad de nombres canónicos, limpieza del alias redundante y de la inyección obsoleta de netcheck; consulta prioritaria del catálogo independiente adm-toolbox, validación del índice remoto y recuperación de la última copia válida o del catálogo local si el remoto es incompatible. Actualizar las fichas al reabrir Guide sin requerir una nueva versión del ejecutable. Conservar todas las fichas válidas, navegación y consulta sin conexión. Pendiente aceptación en Windows bajo autorización. |

## Matriz funcional del catálogo

Los usos son criterios de aceptación, no afirmaciones de que todos estén implementados. Los componentes que requieren una acción, una métrica o una relación no se simulan en producción.

| Elemento | Contexto válido |
|---|---|
| 01 Cabecera asimétrica | Título principal de un informe, medida a su contenido. |
| 02 Divisor escalonado | Límite de secciones relacionadas o tabla, solo cuando mejora la lectura. |
| 03 Marco recortado | Región interactiva real o transferencia, no todos los paneles. |
| 04 Agrupador de campos | Registro de propiedades vinculadas, margen reservado. |
| 05 Pestaña de metadatos | Origen, ámbito o metadato existente de un informe. |
| 06 Riel y folio | Navegación con índice o folio real. |
| 07 Marcas de registro | Elemento bajo inspección o selección explícita. |
| 08 Foco de teclado | Control que recibe las teclas, no el prompt normal. |
| 09 Fila activa | Registro seleccionado de una tabla interactiva. |
| 10 Selector angular | Opciones reales de navegación o autocompletado. |
| 11 Acciones | Acciones realmente disponibles en el contexto. |
| 12 Expansión | Abrir/cerrar datos adicionales de un registro seleccionado. |
| 13 Radial /20 | Utilización porcentual acotada, con etiqueta y dato exacto. |
| 14 Anillo /8 | Proporción de capacidad con denominador conocido. |
| 15 Escala lineal | Medición acotada, unidad y escala explícitas. |
| 16 Umbral y desviación | Medición con umbral real; no inventar límite para temperaturas. |
| 17 Lectura proporcional | Magnitud normalizada con valor y referencia visibles. |
| 18 Escala de actividad | Actividad medida en el tiempo o en un intervalo conocido. |
| 19 Transferencia angular | Transferencia real, dirección o fase identificable. |
| 20 Progreso segmentado | Operación con progreso determinado. |
| 21 Espera indeterminada | Operación real sin porcentaje disponible. |
| 22 Continuidad y pérdidas | Conteos o secuencia observada, nunca cuadrados sin explicación. |
| 23 Ruta / bifurcación | Relación real entre destinos y siguiente salto/interfaz. |
| 24 Trama / transferencia | Volumen o transferencia observada, no fondo decorativo. |
| 25 Correcto | Resultado explícito verificado. |
| 26 Advertencia | Advertencia explícita o umbral conocido. |
| 27 Fallo | Error confirmado, no simple texto rosa. |
| 28 Sin datos | Fuente ausente, no confundir con cero. |
| 29 Evidencia | Registro inspeccionable que respalda un resultado. |
| 30 Estado dinámico | Estado que cambia según observación o actualización real. |
| 31 Título compacto | Encabezado secundario de sección. |
| 32 Folio numérico | Número de registro, página o posición real. |
| 33 Microtipografía | Fuente, unidad, dato secundario, información auxiliar. |
| 34 Entrada y cursor | Edición normal, sin caja ni línea decorativa. |
| 35 Sugerencias | Candidatos reales del motor de completado. |
| 36 Jerarquía de dato | Composición que prioriza resultados y datos principales. |

## Validación de entrega

Estado actual: cambios de fuente en revisión; no verificados en el ejecutable. La compilación anterior falló al quedar un método de edición sin uso tras retirar el adorno del prompt; se eliminó el método en la revisión de fuente. No se ha ejecutado una nueva prueba. No se considera completada la aceptación visual de los 37 criterios ni de los 36 componentes. La versión publicada sigue siendo .11 hasta que exista una entrega completa.

### Cambios de fuente incorporados en esta revisión

- Composición nativa extraída al formatter; monitores con columnas medidas, grupos expandibles y métricas en regiones propias.
- Continuidad etiquetada por conteos explícitos; relaciones de rutas con agrupación real.
- Medidas escalares con unidad, dispositivo y fuente; sin umbrales ni porcentajes inventados.
- Estados, valores y explicaciones con roles distintos; títulos compactos, prompt limpio y eliminación de marcos/folios genéricos.
- Eliminación del texto de desarrollo en la ayuda concreta de tools y en el informe de sensores.

- Revisión común de tablas: separación de cabecera y datos, límites de registros, alineación numérica y conservación de fragmentos no tabulares. Resúmenes de monitores conservan información auxiliar y anuncian líneas omitidas por espacio.
- Selección de texto limitada a celdas seleccionadas, sin cambiar la geometría de cabeceras compactas ni activar una composición anterior al seleccionar.

- Autocompletado: la captura del usuario es la referencia visual más cercana a lo esperado, no una aceptación definitiva. Se conserva su diseño y se ajustan los controles DETALLE y ENTER / ACEPTAR, ESC / CANCELAR al área interior real del marco recortado (8% de recorte en sus esquinas); se respetan las cinco filas y el funcionamiento del teclado. El resto de los informes aún requiere revisión editorial. Corrección en fuente: 35711e9.

Pendiente de aceptación: ejecutar el binario Windows bajo autorización y revisar todas las familias de salidas, todas las ayudas y las 18 paletas. La revisión de fuente no sustituye esa aceptación ni autoriza una publicación.
