# Showcase integral de Shell Shock Tool.
#
# Uso:
#   examples/10-full-showcase.sh
#   examples/10-full-showcase.sh 10.11.24.0/24
#
# No realiza cambios destructivos.

set -o pipefail

NETWORK="${1:-}"
TMPROOT="${TEMP:-.}"
WORK="$TMPROOT/sst-showcase-$$"
REPORT="sst-showcase-${COMPUTERNAME:-host}.txt"

mkdir "$WORK"

cleanup() {
    rm -rf "$WORK"
}
trap cleanup EXIT

section() {
    printf "\n\n================================================================\n"
    printf "%s\n" "$1"
    printf "================================================================\n"
}

scan_network() {
    if [[ -n "$NETWORK" ]]; then
        net scan "$NETWORK"
    else
        net scan
    fi
}

echo "SST full showcase"
echo "Generando datos en paralelo..."

net ping 127.0.0.1 -c 2 > "$WORK/ping-local.txt" &
p_local=$!
net ping 1.1.1.1 -c 2 > "$WORK/ping-cloudflare.txt" &
p_cf=$!
net ping 8.8.8.8 -c 2 > "$WORK/ping-google.txt" &
p_google=$!

printf "jobs: local=%s cloudflare=%s google=%s\n" "$p_local" "$p_cf" "$p_google"
jobs -l
wait

{
    echo "SHELL SHOCK TOOL — FULL SHOWCASE"
    echo "Equipo : ${COMPUTERNAME:-desconocido}"
    echo "Usuario: ${USERNAME:-desconocido}"
    echo "Fecha  : $(date)"
    echo "Nwash  : $NWASH_VERSION"
    echo "Base   : $NWASH_BASH_BASE"

    section "1. SISTEMA"
    sys info
    sys uptime
    sys memory
    sys disks

    section "2. PROCESOS"
    sys processes

    section "3. WINDOWS"
    echo "--- Servicios activos ---"
    service list --running
    echo
    echo "--- Sesiones ---"
    session users
    echo
    echo "--- Impresoras ---"
    sys printers
    echo
    echo "--- PnP con problemas ---"
    pnp list --problem
    echo
    echo "--- Firewall ---"
    firewall status

    section "4. RED"
    net interfaces
    echo
    net routes
    echo
    net neighbors

    echo
    echo "--- Descubrimiento ---"
    time scan_network

    echo
    echo "--- Inventario desconocido ---"
    device unknown

    section "5. CONECTIVIDAD EN PARALELO"
    cat "$WORK/ping-local.txt"
    cat "$WORK/ping-cloudflare.txt"
    cat "$WORK/ping-google.txt"

    section "6. TRÁFICO Y FIRMAS"
    net traffic --top 15
    echo
    net traffic --unsigned --top 15

    section "7. SEGURIDAD"
    triage
    echo
    sys suspicious
    echo
    sys startup
    echo
    sys persistence

    section "8. INTEL"
    intel status
    intel sources

    section "9. EVENTOS"
    eventlog read System --count 20

    section "10. CONFIGURACIÓN Y UI"
    config path
    config bg
} > "$REPORT"

echo
echo "=== Resultado ==="
realpath "$REPORT"

echo
echo "Líneas/palabras/bytes:"
wc "$REPORT"

echo
echo "SHA-256:"
sha256sum "$REPORT"

echo
echo "Primeras líneas:"
head "$REPORT"

echo
echo "=== Capacidades demostradas ==="
cat <<'EOF'
- scripting Bash-compatible nativo;
- funciones y parámetros;
- jobs en background;
- espera de procesos;
- redirecciones y reportes;
- utilidades Unix integradas;
- administración Windows;
- descubrimiento de red;
- inventario de dispositivos;
- ETW/tráfico y Authenticode;
- triage de seguridad;
- fuentes Intel;
- configuración portable y fondos.
EOF

echo
echo "Reporte completo guardado en $REPORT"
