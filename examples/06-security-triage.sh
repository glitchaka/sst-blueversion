# Snapshot de seguridad local no destructivo.

set -o pipefail
REPORT="sst-security-${COMPUTERNAME:-host}.txt"

section() {
    printf "\n\n===== %s =====\n" "$1"
}

{
    echo "SST SECURITY TRIAGE"
    echo "Equipo : ${COMPUTERNAME:-desconocido}"
    echo "Usuario: ${USERNAME:-desconocido}"
    echo "Fecha  : $(date)"

    section "Triage"
    triage

    section "Procesos sospechosos según reglas locales"
    sys suspicious

    section "Inicio automático"
    sys startup

    section "Persistencia"
    sys persistence

    section "Servicios con impacto"
    sys services --impact

    section "Procesos con firma no válida / desconocida"
    net traffic --unsigned --top 20

    section "Estado de fuentes Intel"
    intel status
    intel sources

    section "Eventos de sistema recientes"
    eventlog read System --count 30
} > "$REPORT"

echo "Reporte de triage:"
realpath "$REPORT"
echo
echo "Resumen inicial:"
head "$REPORT"
