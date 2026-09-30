#!/usr/bin/env nwash
# Demostración de configuración portable y fondos.
# No cambia el fondo automáticamente: muestra los comandos disponibles.

echo "=== Archivo de configuración ==="
config path

echo
echo "=== Fondos disponibles ==="
config bg

cat <<'EOF'

Ejemplos:

  config bg 1
      Selecciona la primera imagen de bg/.

  config bg paisaje.jpg
      Fija una imagen por nombre.

  config bg carrousel
      Activa carrusel a 3 minutos.

  config bg carrousel 5
      Activa carrusel a 5 minutos.

  config bg next
      Avanza manualmente al siguiente fondo y lo deja fijo.

  config bg off
      Desactiva la imagen de fondo.

En modo carrousel SST también rota al:
  - entrar o salir de Helix;
  - entrar o salir de net monitor / sys top / otras TUIs;
  - minimizar/restaurar;
  - maximizar/restaurar.
EOF
