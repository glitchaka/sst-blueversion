# SST — banda sonora de `keygen`

`keygen` usa el reproductor nativo WinMM/MCI de Windows. No ejecuta programas externos ni deja un reproductor residente.

## Instalación sin pasos manuales

Una distribución SST que incluya la banda sonora coloca automáticamente las dos pistas en `data/.sst/media/keygen/` durante el empaquetado SFX. El paquete portátil conserva la misma carpeta.

Para reparar instalaciones antiguas, SST también localiza y valida las dos pistas originales en **Descargas**, con los nombres suministrados por el autor, o en los archivos `sst-keygen-soundtrack.zip` y `SST-Keygen-Autoinstalador.zip`. Si las encuentra, instala una copia verificada en `%LOCALAPPDATA%\SST\data\.sst\media\keygen`, sin pedir al operador que cree directorios o copie archivos.

Los recursos se verifican mediante SHA-256 antes de reproducirlos. No hay descarga automática desde servicios de terceros ni sustitución por audio inventado. Para un directorio administrado explícitamente se admite `SST_KEYGEN_MUSIC_DIR`.

El código fuente de SST y el empaquetado están preparados para incluir las pistas cuando se suministran como assets de compilación. **La ausencia de los MP3 en el repositorio de origen impide que una release pública incorpore esas pistas**: no debe declararse lo contrario.

## Uso

```text
keygen
keygen --track 2
keygen --help
```

Al abrir `keygen`, empieza el audio y se muestra la animación. Al finalizar cada pista cambia a la siguiente, en bucle.

- **1** y **2**: cambiar la pista.
- **Espacio**: pausar/reanudar sonido y animación.
- **Esc** o **q**: cerrar y detener el reproductor.

## Verificación y seguridad

La recuperación compara SHA-256 con los dos archivos originales suministrados; nunca acepta un MP3 diferente con el mismo nombre. El destructor de `KeygenMusic` ejecuta `stop` y `close` al salir, incluidos los errores. El empaquetador rechaza un conjunto parcial de pistas.

La verificación de la lógica Rust no sustituye una prueba de reproducción real en Windows.
