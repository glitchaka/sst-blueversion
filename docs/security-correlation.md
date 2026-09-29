# Correlación y priorización

El motor puro de `src/core/triage.rs` recibe evidencia por familias y devuelve
`NORMAL`, `PERFORMANCE`, `ATTENTION`, `SUSPICIOUS` o `ALERT`, razones y limitaciones.
Cada nueva evaluación reemplaza la conclusión anterior: no acumula puntajes.

- Una observación débil aislada no eleva la clasificación.
- Dos familias débiles o una anomalía concreta producen `ATTENTION`.
- Tres familias independientes, incluyendo una anomalía, producen `SUSPICIOUS`.
- Una evidencia `Decisive`, reservada a coincidencias maliciosas exactas verificadas,
  produce `ALERT`. También lo hacen dos familias independientes `Strong`.
  Una señal `Strong` aislada produce `ATTENTION`, nunca `ALERT` por sí sola.
  Semejanza de nombre y ausencia de reputación son contexto.
- Recursos nunca contribuyen a la severidad de seguridad. Sin evidencia de
  seguridad suficiente, consumo relevante produce `PERFORMANCE`.
- Sólo contribuyen observaciones `KNOWN`. `UNKNOWN`, `PENDING`, `ACCESS_DENIED`
  y `UNAVAILABLE` se conservan como limitaciones.
- Firma válida e historial estable no cancelan evidencia actual.

El preload, `sys suspicious` y `sys why` usan las mismas reglas de seguridad.
La integración actual observa ruta, ejecución PowerShell y linaje. Compara el
ejecutable padre con los padres observados históricamente, evitando comparar PIDs
entre ejecuciones. La primera aparición por sí sola no es una anomalía.
El perfil cuenta ejecuciones distintas por PID + start_time, excluye instancias
actuales y utiliza observaciones de los últimos 90 días. Con 0–2 ejecuciones
el perfil es insuficiente y no eleva por padre nuevo. Con 3–19, un padre nuevo
aporta `Weak`. Desde 20, un padre nuevo o con frecuencia inferior al 5% aporta
`Anomaly`. Estos umbrales son una política inicial explícita.
`sys diff` muestra cambios de padre y de línea de comandos disponibles.

`sys why` representa directamente el `Assessment`: clasificación, razones y
limitaciones, incluso para `NORMAL` y `PERFORMANCE`. Los datos ausentes y el
historial insuficiente entran al motor como observaciones incompletas. Los
recolectores no conectados se declaran `UNAVAILABLE`, no `PENDING`. Los hallazgos
conservan las limitaciones y `sys suspicious` también las muestra.

`correlation_history` conserva clasificación y razones por PID, hora de inicio e
identidad de ruta. Sólo agrega filas cuando cambia clasificación o explicación;
las escrituras son asíncronas y una falla de la base no impide el análisis local.

Los recolectores de firma, SHA-256, conexiones, persistencia y reputación siguen
diferidos en el esqueleto existente. `sys why` lo indica. El contrato del motor
acepta esas familias y permite recalcular al recibirlas; esta implementación no
consulta servicios externos ni calcula hashes durante el preload. La confianza
por identidad de hash requiere que el recolector proporcione un hash verificado;
el historial de rutas actual no equivale a identidad criptográfica.
`compare_hashes` permite aportar un cambio de SHA-256 como evidencia `Strong`
de identidad. Requiere dos digests válidos; su ausencia no implica cambio.
El recolector y almacenamiento de esos digests aún están pendientes.

Pruebas disponibles: `cargo test core::triage` y `cargo test correlation_tests`.
Ejecutarlas requiere autorización explícita del usuario.
