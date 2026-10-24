# Distribución y autoupdate de SST

`sst-blueversion` es el repositorio fuente privado. No debe usarse como canal de actualización para usuarios finales: los assets y `raw.githubusercontent.com` de un repositorio privado requieren autenticación de GitHub.

## Artefactos de una versión

El workflow manual **Release SST** genera en `dist/`:

- `sst.exe`: ejecutable actualizable.
- `SST-Setup-<version>.exe`: autoextraíble portable.
- `sst-update.json`: manifiesto con versión, SHA-256 y tamaño de `sst.exe`.
- `version.txt`: versión canónica tomada de `Cargo.toml`.

`SST-Setup-<version>.exe` se copia a un pendrive y, al ejecutarse, extrae el payload en una carpeta `SST` situada en ese mismo soporte. La futura integración de `disk-seal` se hará sobre este flujo; no forma parte todavía del autoextraíble.

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
