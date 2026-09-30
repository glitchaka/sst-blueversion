#!/usr/bin/env nwash
# Snapshot de red + comparación temporal.
# Uso:
#   examples/03-network-discovery.sh
#   examples/03-network-discovery.sh 10.11.24.0/24

set -o pipefail

NETWORK="${1:-}"
TMPROOT="${TEMP:-.}"
BEFORE="$TMPROOT/sst-net-before-$$.txt"
AFTER="$TMPROOT/sst-net-after-$$.txt"

cleanup() {
    rm -f "$BEFORE" "$AFTER"
}
trap cleanup EXIT

scan_names() {
    if [[ -n "$NETWORK" ]]; then
        net scan "$NETWORK" --names
    else
        net scan --names
    fi
}

echo "=== Interfaces ==="
net interfaces

echo
echo "=== Rutas ==="
net routes

echo
echo "=== Vecinos enriquecidos ==="
net neighbors

echo
echo "=== Snapshot inicial ==="
scan_names | tee "$BEFORE"

echo
echo "Esperando 5 segundos para detectar cambios..."
sleep 5

echo
echo "=== Segundo snapshot ==="
scan_names | tee "$AFTER"

echo
echo "=== Diferencias ordenadas ==="
diff <(sort "$BEFORE") <(sort "$AFTER") || true

echo
echo "=== Equipos vistos pero aún no inventariados ==="
device unknown

echo
echo "Consejo:"
echo "  net identify IP"
echo "  device add IP NOMBRE --note \"descripción\""
echo "  net monitor ${NETWORK:-RED}   # Enter abre el prompt de mensajes LAN"
