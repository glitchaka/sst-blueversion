# Estado de implementación — SST Security Triage

Revisión de código estática, 4 de noviembre de 2026.
Fuente de diseño: [SECURITY_TRIAGE_PLAN.md](SECURITY_TRIAGE_PLAN.md).
Fuente de implementación: `src/application/security.rs`, componentes
`security_deferred`, `README.md` y comandos registrados en SST.

**Implementado en el código (no garantiza todos los entornos):**

| Capacidad | Dónde se observa | Limitación |
| --- | --- | --- |
| Preload rápido | `run_startup_preload` | Procesos, PID/PPID, memoria, CPU/I/O; sin esperar correlaciones lentas. |
| Recopilación diferida | `security_deferred::start`, `enrich`, `status` | Conexiones, persistencia, servicios, firmas/hashes; puede estar en curso o sin datos según permisos/proveedores. |
| Diagnóstico manual | `triage`, `triage --deep`, `triage PID`, `triage --status` | Las muestras representan un instante y los enriquecimientos pueden llegar después. |
| Explicación por proceso | `why`, `inspect`, `diff`, `suspicious` | Sin evidencia suficiente se debe informar incertidumbre, no certificar que un proceso es seguro. |
| Historial y confianza local | `historical_parents`, `learned_lineages`, `safe` | Una relación conocida sólo rebaja novedad histórica, no ignora otras señales. |
| Métricas de rendimiento separadas | `analyze_performance`, `triage_memory_report` | Alto consumo de recursos no significa automáticamente malware. |
| Inteligencia opcional | `intel status/sources/update/lookup` | Dependiente de fuentes activadas, red y credenciales; no garantiza reputación universal. |

**Pendientes, parciales o que requieren validación:**

- `sys inspect PID --deep` declara expresamente que módulos, memoria y handles
  quedan diferidos; no presentarlo como inspección forense completa.
- La correlación diferida debe verificarse con casos reales en Windows con
  y sin privilegios de administrador y con proveedores ausentes.
- Los flujos de broker privilegiado requieren pruebas controladas de permisos,
  identidad PID+FILETIME y cancelación; no se infiere su éxito sólo del código.
- Cobertura de tests: añadir escenarios benignos, PID reciclados,
  ausencia de Authenticode, fallos de fuentes y persistencia desconocida.
- No existe decisión automática de terminar procesos como resultado de un
  hallazgo de triage; acciones destructivas requieren intervención del operador.

**Demostración:** usar `triage`, `triage --status`, `triage PID`,
`sys why PID` y `sys inspect PID` sobre procesos conocidos y verificar
que no se produzcan acciones destructivas no solicitadas. Describir
resultados como diagnósticos, no como detecciones certificadas.
