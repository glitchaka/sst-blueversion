# SST Formatter — contrato de presentación

Todos los resultados interactivos completos atraviesan Formatter::result una sola vez. El motor Bash devuelve datos sin decoración. stdout, stderr, líneas en vivo y monitores utilizan el módulo src/presentation/formatter.rs; report y tui_style son fachadas de compatibilidad sin paletas propias.

Roles: título, comando, etiqueta, detalle, valor, éxito, aviso, fallo y óptimo. El tema controla color, peso, escala tipográfica y acentos gráficos. La terminal mantiene una cuadrícula monoespaciada; los tamaños se ajustan a cada celda para evitar superposición. Slint pinta reglas y acentos reales para títulos semánticos; no hay cajas de caracteres.

OutputMode::Machine preserva CSV, JSON, scripts, redirecciones, tuberías y herramientas de texto. OutputMode::Terminal preserva protocolos VT, aplicaciones interactivas y el arte original. El formatter ofrece Document para nuevos informes semánticos y un adaptador de texto para los comandos existentes. En informes humanos el formatter retira los estilos SGR antiguos antes de aplicar el tema. Las secuencias de control de terminal se conservan; la migración de sus productores a Document debe realizarse sin cambiar los datos ni romper Bash. El diagnóstico NOTSH y la salida AD se organizan sin inferir estados no demostrados.

El texto base usa verde amarillo (#B8BB26), los comandos naranja (#FE8019), los títulos ámbar (#FABD2F) y las etiquetas verde agua (#83A598). Las descripciones usan crema (#EBDBB2). Los colores de estado se asignan a valores explícitos.
