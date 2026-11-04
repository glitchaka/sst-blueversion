# SST — revisión estática y pendientes verificables (4 de noviembre de 2026)

Repositorio: `glitchaka/sst-blueversion` · versión del manifiesto: `0.1.9-blue.8`.

**Este archivo no es un certificado de seguridad.** Distingue evidencia de código,
validaciones ejecutadas y pendientes. No marcar como resuelto por el solo hecho
de tener un build verde o de estar documentado. La publicación de una nueva
release exige validación propia del artefacto.

## Evidencia reproducible

| Área | Evidencia en repositorio | Estado y límite |
| --- | --- | --- |
| Windows build y tests Rust | `.github/workflows/build.yml` | Existe CI; el resultado aplica al commit exacto ensayado. |
| Shell black-box y estrés | `.github/workflows/terminal-blackbox-stress.yml` | Debe aprobar el conjunto completo antes de declarar cierre funcional; incluye la medición de GUI. |
| Triage | `src/application/security.rs` y componentes `security_deferred` | Hay código integrado; ver `SECURITY_TRIAGE_STATUS.md` para límites operativos. |
| Actualizador | `.github/workflows/self-update-regression.yml`, `release.yml` | SHA-256 de payload no equivale a integridad de *assets de build*. |
| Guía | `guide/` y workflow `guide-live-regression.yml` | Diferenciar recuperación offline existente de la hoja de ruta por apartados de blue.9. |

## Hallazgos abiertos: no confundirlos con CI verde

1. **Integridad de insumos del build — ABIERTO.** `build.rs` descarga
   Nerd Font, Helix, es-CL e IEEE con HTTPS y tamaño mínimo, pero no verifica
   un SHA-256 esperado. Las listas IEEE pueden variar fuera de CI.
   Hay que establecer digests de fuentes contrastadas, guardarlos en un manifiesto
   revisado y comprobar tanto nuevas descargas como caché y overrides;
   los workflows de release deben fallar ante cualquier discrepancia. No generar
   el digest esperado desde el mismo archivo recién descargado.
2. **Dependencias — AUDITORÍA AÑADIDA, HALLAZGOS POR REVISAR.**
   `Cargo.toml` incluye `zip = "0.6"` y `toml = "0.5"`.
   El workflow `dependency-audit.yml` ejecuta `cargo audit --file Cargo.lock`.
   Un resultado con advisories requiere análisis de impacto y actualización
   focalizada: no migrar versiones mayores sin revisar código/ABI y pruebas.
3. **Cobertura Nwash — ABIERTO.** `src/core/bash/interpreter.rs` concentra
   mucha lógica y una prueba exitosa del parser no valida todas las rutas.
   Aumentar tests de redirecciones, pipelines, expansión y scripts reales.
   Complementar con pruebas black-box y diferenciales, no sustituirlas.
4. **Auditoría Win32 — ABIERTO.** Revisar contextualmente los bloques
   `unsafe`, contratos de punteros/longitudes, propiedad de handles y
   `.unwrap()` en rutas que procesan entradas o estados externos.
   Los recuentos estáticos, sin inspección individual, no prueban vulnerabilidades.
5. **Clippy y formato — DEUDA VISIBLE.** En `build.yml` ambos son
   `continue-on-error: true`, incluso Clippy con `-D warnings`.
   El build verde no implica ausencia de avisos. Normalizar deuda antigua
   por módulos y luego retirar esa excepción.
6. **Planes — ESTADO POR COMPONENTE DOCUMENTADO.** El diseño
   `SECURITY_TRIAGE_PLAN.md` no debe leerse como lista de funciones implementadas;
   ver `SECURITY_TRIAGE_STATUS.md`.
7. **Blue.9 — HOJA DE RUTA ABIERTA.** Los 34 elementos de `TODO.md` son
   trabajo pendiente de blue.9, no características verificadas de blue.8.

## Comprobaciones de CI verificadas el 4 de noviembre

- Build Windows [#17](https://github.com/glitchaka/sst-blueversion/actions/runs/41745352492),
  commit `23a8b53`: **éxito** (pruebas y build). El formato y Clippy siguen
  siendo pasos informativos.
- Black-box [#16](https://github.com/glitchaka/sst-blueversion/actions/runs/41745337354):
  **196/197**, fallo específico de `cut -d: -f2`; corrección posterior en
  `6149951`, pendiente de validar en la ejecución completa.
- Build Windows [#18](https://github.com/glitchaka/sst-blueversion/actions/runs/41848658332):
  fallo por timeout de descarga en vivo IEEE dentro de `build.rs`, no por
  un error de compilación Rust. El workflow fue modificado en `b6221da`
  para usar el snapshot IEEE fijado como en black-box; requiere nueva ejecución.
- `v0.1.9-blue.8` conserva artefactos de publicación anteriores (30 de
  octubre); **no representa automáticamente las correcciones de `main`**.

El estado de cada run puede cambiar. Comprobar su conclusión y SHA antes
de actualizar estas afirmaciones.

## Puerta de calidad para una demostración y release

- Compilar y pasar pruebas Rust en Windows en el **commit distribuido**.
- Aprobar black-box, estrés y consumo GUI, sin cambiar el umbral para lograr verde.
- Pasar empaquetado SFX y comparar SHA-256 del ejecutable extraído.
- Revisar resultado real de RustSec; registrar y resolver advisories relevantes.
- Ejecutar comprobación del instalador portable, arranque y actualización en un
  Windows de prueba. Hasta entonces: **release no certificada por esta auditoría**.

La mitigación de hashes de assets sigue abierta hasta contar con digests confiables
y pruebas de fallo de descarga/caché. Nunca presentar un control opcional
no configurado como protección aplicada.
