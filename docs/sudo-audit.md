# Auditoría sudo de SST

La versión de desarrollo blue.26 escribe eventos de solicitud y resultado en data/sudo-audit.jsonl. Cada par comparte un identificador único y contiene fecha, hora, zona horaria, equipo, usuario, nivel real de sesión, categoría de operación y código de salida. No captura argumentos ni tokens de seguridad.

No transmite todavía estos datos a GitHub. El repositorio glitchaka/adm-toolbox es público; no corresponde enviar allí identidades de usuarios o equipos. Para activar la auditoría central es necesario un destino privado, un receptor autenticado y credenciales restringidas almacenadas fuera del ejecutable. Se requieren confirmación de recepción e idempotencia por event_id. Los registros locales no están protegidos contra manipulaciones y el fallo de escritura no bloquea por ahora sudo.
