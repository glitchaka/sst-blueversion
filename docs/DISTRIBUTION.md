# Distribución y autoupdate de SST

`sst-blueversion` es el repositorio fuente privado. No debe usarse como canal de actualización para usuarios finales: los assets y `raw.githubusercontent.com` de un repositorio privado requieren autenticación de GitHub.

## Artefactos de una versión

El workflow manual **Release SST** genera en `dist/`:

- `sst.exe`: ejecutable actualizable.
- `SST-Setup-<version>.exe`: autoextraíble portable de un solo uso.
- `sst-update.json`: manifiesto con versión, SHA-256 y tamaño de `sst.exe` y metadatos del setup.
- `version.txt`: versión canónica tomada de `Cargo.toml`.

`SST-Setup-<version>.exe` usa el formato `SSTSFX03`. Cada paquete recibe al construirse un ID aleatorio de 128 bits que también se publica en `setup.package_id` dentro de `sst-update.json`.

El setup está diseñado para una sola extracción:

1. Antes de extraer reserva de forma atómica el ID del paquete para impedir ejecuciones simultáneas.
2. Si la extracción falla antes de completarse, libera la reserva y permite reintentar el mismo paquete.
3. Cuando `sst.exe` y el resto del payload terminan de escribirse correctamente, el ID queda marcado como consumido en el equipo.
4. Después del éxito se inicia un helper oculto que espera a que cierre el stub, trunca el autoextraíble y lo elimina.
5. Si otra copia con el mismo ID intenta ejecutarse en ese equipo, se rechaza sin extraer y también programa su eliminación.

El registro de consumo intenta usar primero `%ProgramData%\Shell Shock Tool\sfx-consumed`; si no existe permiso de escritura usa `%LocalAppData%` y, como último recurso, la carpeta temporal del usuario. De esta forma el autoextraíble no depende de un archivo sidecar situado junto al paquete.

La garantía anterior es local/offline. Una copia realizada antes del consumo y llevada a otro equipo no puede quedar invalidada globalmente sin un servicio de canje o activación en red. Ese control global podrá integrarse con el sistema de activación cuando corresponda.

En el uso portable normal el setup puede copiarse a un pendrive y, al ejecutarse, extrae el payload en una carpeta `SST` situada en ese mismo soporte; después consume y elimina esa copia del autoextraíble. La futura integración de `disk-seal` se hará sobre este flujo.

## Verificación del SFX

`.github/workflows/sfx-e2e.yml` es manual. La verificación crea dos copias temporales del mismo setup antes de ejecutarlo:

- la primera debe extraer correctamente y autoeliminarse;
- la segunda debe ser rechazada por compartir el mismo `package_id` y no puede dejar un segundo `sst.exe`;
- el archivo original dentro de `dist/` no se ejecuta durante la prueba y queda disponible para publicación.

El workflow de release usa el mismo principio: verifica una copia descartable para no consumir el artefacto que luego se publica.

## Canal público

El código fuente puede seguir privado, pero las actualizaciones destinadas a usuarios normales necesitan un endpoint HTTPS público. El canal público sólo necesita exponer los artefactos anteriores; nunca debe contener el código fuente ni claves privadas.

El updater busca la URL del manifiesto en este orden:

1. variable de entorno `SST_UPDATE_MANIFEST_URL`;
2. `data/update-channel.txt` junto a la instalación portable;
3. valor `SST_UPDATE_MANIFEST_URL` embebido en tiempo de compilación.

Al empaquetar se puede pasar `-UpdateManifestUrl` a `scripts/package-release.ps1`; el autoextraíble incluirá entonces `data/update-channel.txt` en la instalación.

`asset_url` puede ser una URL HTTPS absoluta o un nombre de archivo simple como `sst.exe`. En el segundo caso SST lo resuelve en la misma carpeta HTTPS donde está `sst-update.json`.

## Flujo de autoupdate

1. SST consulta silenciosamente el manifiesto al iniciar la GUI.
2. Si la versión remota es superior, descarga `sst.exe` a `data/update/`.
3. Comprueba tamaño (si está presente) y SHA-256.
4. Registra la actualización como pendiente.
5. Al cerrar SST se inicia un helper temporal oculto basado en `cmd.exe` del sistema.
6. El helper espera a que `sst.exe` quede libre, sustituye el binario, relanza SST y se elimina.

No se instala un `updater.exe` permanente.

También están disponibles los comandos:

```text
update status
update check
update install
```

## Versión canónica

`Cargo.toml` es la única fuente de verdad. `scripts/sync-version.ps1` mantiene la entrada raíz de `Cargo.lock` alineada. El empaquetado, nombres de assets y manifiesto leen la misma versión.
