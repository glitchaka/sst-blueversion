# Documentación de SST y guía original

El comando `guide` utiliza como fuente principal `glitchaka/adm-toolbox`, directorio `tutorials/` de la rama `main`:

https://github.com/glitchaka/adm-toolbox/tree/main/tutorials

Al abrir `guide`, SST solicita el índice completo del repositorio original, que incorpora la sección de Arch. Los manuales se descargan bajo demanda al seleccionar una ficha. La descarga solo admite HTTPS, comprueba límites de tamaño y exige texto UTF-8; el índice también se valida como JSON con esquema 1.

Los archivos procedentes del repositorio original se guardan en `guide/upstream/`, separadamente de los manuales locales del usuario. Si falla la conexión, se reutiliza el último índice/manual descargado; en ausencia de una copia del origen se utiliza la documentación local incluida con SST. Los archivos del usuario no se sobrescriben.

La instantánea de SST en `guide/` sigue disponible para funcionamiento sin red. Los nuevos contenidos de Arch requerirán haberse consultado previamente en línea para estar disponibles sin conexión.
