# SST — banda sonora de `keygen`

El comando `keygen` utiliza el reproductor nativo WinMM/MCI de Windows: no ejecuta programas externos ni deja un proceso de audio residente.

## Instalación de pistas

Los MP3 se distribuyen **separadamente** del repositorio público, por tratarse de archivos musicales proporcionados por el usuario.

```text
sst.exe
assets/
  keygen/
    track01.mp3  # The Best Keygen Song Ever! — Kinespojken
    track02.mp3  # Keygen Music By FFF — Foundeer
```

La carpeta puede cambiarse mediante la variable de entorno `SST_KEYGEN_MUSIC_DIR`.

## Uso

```text
keygen
keygen --track 2
keygen --help
```

Al abrir `keygen`, empieza el audio y se muestran los textos animados. Al finalizar cada pista cambia a la siguiente, en bucle.

- **1** y **2**: cambiar la pista inmediatamente.
- **Espacio**: pausar/reanudar sonido y animación.
- **Esc** o **q**: cerrar el modo y detener/descargar el dispositivo de audio.

Se informa si faltan los recursos. En cualquier ruta de salida (incluidos errores), el destructor de `KeygenMusic` ejecuta `stop` y `close` del alias MCI. El audio no sigue sonando tras cerrar el comando.

## Verificación pendiente

El código requiere compilación y prueba real en Windows con los dos MP3 y el renderer Slint/VT100 de SST. La presencia del código en `main` no equivale a una versión publicada o verificada.
