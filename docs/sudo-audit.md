# SST: auditoría remota obligatoria para privilegios SYSTEM y TrustedInstaller

## Receptor provisional

Se utiliza el repositorio público glitchaka/adm-toolbox mediante la API de GitHub Issues. Cada solicitud de operación privilegiada debe obtener una confirmación de creación (número y URL de issue) antes de ejecutar; sin confirmación, conexión o credencial, SST deniega la operación. El resultado se intenta registrar como comentario en el issue y, si no se puede, se guarda la situación pendiente en la auditoría local (la acción ya pudo haber ocurrido).

## Aprovisionamiento de credencial

Cada cuenta elevada que vaya a operar SST debe disponer de una credencial genérica en el Administrador de credenciales de Windows con destino SST/GitHubAudit. Su contraseña será un token GitHub de permisos mínimos, limitado al repositorio glitchaka/adm-toolbox y con permiso Issues: write. No almacenar tokens en la memoria USB, configuraciones del repositorio, variables de entorno ni argumentos de procesos. Si no existe credencial, SYSTEM y TrustedInstaller se bloquean. El token puede ser revocado desde GitHub. Las cuentas elevadas distintas necesitan su propia credencial.

## Campos de auditoría

Identificador de evento, fecha, hora, zona horaria, equipo, usuario Windows, nivel de token, operación, nombre del ejecutable, estado y versión SST. No se almacenan argumentos completos, contraseñas ni tokens Windows. El registro local sigue en data/sudo-audit.jsonl.

## Limitaciones

El repositorio es público y los registros de nombres de usuario y equipo serán públicos, incluso después de migrar el receptor. Issues no es almacenamiento inmutable y quienes tengan permisos sobre el repositorio pueden alterar registros. Sin la ejecución de una operación supervisada por un servicio con identidad confiable y registro de recurso afectado (antes y después), esto registra solicitudes, no demuestra la eliminación o modificación real. La falta de conectividad impide iniciar operaciones privilegiadas dentro de SST, pero no impide que un administrador use herramientas externas. Los fallos al enviar el resultado después de una operación deben ponerse en cola y reconciliarse con un receptor fiable antes de considerar la auditoría completa.

## Publicación

Los cambios de desarrollo NO se deben distribuir mediante el canal automático sin informar de la versión y los requisitos de credencial. No se incluye disparador de publicación en este cambio.
