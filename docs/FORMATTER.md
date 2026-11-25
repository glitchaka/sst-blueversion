# SST Formatter — contrato de presentación

Todos los resultados interactivos completos atraviesan Formatter::present una sola vez. El motor Bash devuelve datos sin decoración. stdout, stderr, líneas en vivo y monitores utilizan el módulo src/presentation/formatter.rs; report y tui_style son fachadas de compatibilidad sin paletas propias.

Roles: título, comando, etiqueta, detalle, valor, éxito, aviso, fallo y óptimo. El tema controla color, peso, escala tipográfica y acentos gráficos. La terminal mantiene una cuadrícula monoespaciada; los tamaños se ajustan a cada celda para evitar superposición. Slint pinta reglas y acentos reales para títulos semánticos; no hay cajas de caracteres.

OutputMode::Machine preserva CSV, JSON, scripts, redirecciones, tuberías y herramientas de texto. OutputMode::Terminal preserva protocolos VT, aplicaciones interactivas y el arte original. El formatter ofrece Document para nuevos informes semánticos y un adaptador de texto para los comandos existentes. En informes humanos el formatter retira los estilos SGR antiguos antes de aplicar el tema. Las secuencias de control de terminal se conservan; la migración de sus productores a Document debe realizarse sin cambiar los datos ni romper Bash. El diagnóstico NOTSH y la salida AD se organizan sin inferir estados no demostrados.

El texto base usa verde amarillo (#B8BB26), los comandos naranja (#FE8019), los títulos ámbar (#FABD2F) y las etiquetas verde agua (#83A598). Las descripciones usan crema (#EBDBB2). Los colores de estado se asignan a valores explícitos.

La selección y el cursor son capas globales de la ventana. El cursor se anima sin redibujar la cuadrícula. Una selección en una fila acentúa la fila completa y mantiene legibles las alertas. El formatter reconoce las columnas por su cabecera y los campos por nombre y unidad. CPU al 25 % genera un aviso y al 50 % una alerta; en muestras sucesivas del mismo proceso una subida de 25 puntos también activa la alerta. Utilización de memoria o disco al 75/90 % y pérdida de paquetes al 0,1/5 % usan aviso/alerta respectivamente. Valores en MiB no son porcentajes. Una instantánea se colorea al recibirla y los monitores evalúan cada muestra nueva.

## Composición editorial

El componente feature_grid separa etiquetas, espacios de reserva y valores; utiliza dos columnas cuando el ancho permite leer ambos valores y se apila en ventanas estrechas. El renderizador nativo emplea un tamaño de exhibición de hasta 1,65 veces el tamaño base, limitado por la anchura de cada región y dos filas de altura. El adaptador NOTSH prioriza el resultado y las mediciones observadas, seguido de comprobaciones. Los campos corrientes no son cifras de exhibición: desde cuatro campos compactos pueden ocupar una retícula de dos regiones sin fondo; en anchos reducidos vuelven a una columna. Un resultado explícito al inicio de un informe tiene prioridad visual sobre sus metadatos. Las tablas miden cada columna y se repliegan por celda, o como registros etiquetados en ventanas muy estrechas. Los monitores conservan la identidad de las columnas entre muestras. La salida original para máquinas no cambia. Se mantiene el tema existente, sin copiar las fuentes ni motivos decorativos de las referencias.

## Jerarquía sin paneles indiscriminados

El texto ordinario y los grupos de campos no pintan bandas ni gutters. Únicamente el título inicial y las cifras destacadas reservan filas tipográficas; sus marcadores de layout son transparentes en la ventana nativa. Las tablas de datos y las barras de métricas conservan fondos funcionales. El diagnóstico muestra primero el resultado, seguido por metadatos, mediciones y evidencia. Se conserva el fondo y la paleta de SST.

## Contrato independiente del productor

La selección de composición se basa exclusivamente en la estructura del texto: referencia, evaluación con conclusión, resumen con conteo explícito, informe, o texto simple. El nombre del comando y el registro de comandos no intervienen en la selección ni en la presentación. Solo el enrutamiento de transporte distingue salida de máquina y protocolos VT. Una misma salida interactiva produce el mismo diseño independientemente de su productor. Pruebas de equivalencia cubren herramientas conocidas y desconocidas.

## Lectura del resultado en la ventana nativa

Los resultados humanos completos viajan por el mismo canal ordenado con límites de presentación tipados. Si el comienzo de un resultado sale del viewport, la ventana se posiciona en ese comienzo después de imprimir el prompt; no elimina el historial. La rueda y PageUp/PageDown permiten seguir leyendo; una nueva entrada regresa al prompt. Las aplicaciones de pantalla completa, la salida incremental y los modos de máquina no adoptan esta política. El usuario que ya está desplazado conserva el control de la lectura.

Las referencias de ayuda con título descriptivo usan la misma cabecera editorial de los informes, a escala de exhibición y sobre fondo transparente; las subsecciones y descripciones mantienen su tamaño compacto. No se introduce otra fuente, color ni marco.
