# SST Formatter — contrato de presentación

Todos los resultados interactivos completos atraviesan Formatter::result una sola vez. El motor Bash devuelve datos sin decoración. stdout, stderr, líneas en vivo y monitores utilizan el módulo src/presentation/formatter.rs; report y tui_style son fachadas de compatibilidad sin paletas propias.

Roles: título, comando, etiqueta, detalle, valor, éxito, aviso, fallo y óptimo. El tema controla color, peso, escala tipográfica y acentos gráficos. La terminal mantiene una cuadrícula monoespaciada; los tamaños se ajustan a cada celda para evitar superposición. Slint pinta reglas y acentos reales para títulos semánticos; no hay cajas de caracteres.

OutputMode::Machine preserva CSV, JSON, scripts, redirecciones, tuberías y herramientas de texto. OutputMode::Terminal preserva protocolos VT, aplicaciones interactivas y el arte original. El formatter ofrece Document para nuevos informes semánticos y un adaptador de texto para los comandos existentes. Los comandos que todavía producen ANSI o texto estructurado conservan compatibilidad; la migración de sus productores a Document debe realizarse sin cambiar los datos ni romper Bash. El diagnóstico NOTSH y la salida AD se organizan sin inferir estados no demostrados.
