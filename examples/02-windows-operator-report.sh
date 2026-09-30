#!/usr/bin/env nwash
# Informe operativo no destructivo de Windows usando builtins SST/Nwash.

set -o pipefail
REPORT="sst-operator-${COMPUTERNAME:-host}.txt"

section() {
    printf "\n\n===== %s =====\n" "$1"
}

{
    echo "SST OPERATOR REPORT"
    echo "Equipo : ${COMPUTERNAME:-desconocido}"
    echo "Usuario: ${USERNAME:-desconocido}"
    echo "Fecha  : $(date)"

    section "Sistema"
    sys info
    sys uptime
    sys memory
    sys disks

    section "Procesos"
    sys processes

    section "Servicios activos"
    service list --running

    section "Sesiones"
    session users

    section "Impresoras"
    sys printers

    section "Dispositivos PnP con problemas"
    pnp list --problem

    section "Tareas programadas"
    task list

    section "Firewall"
    firewall status

    section "Shares SMB"
    share list

    section "Eventos System recientes"
    eventlog read System --count 20

    section "Red"
    net interfaces
    net routes
    net neighbors

    section "Resumen de seguridad"
    triage
} > "$REPORT"

echo "Informe generado:"
realpath "$REPORT"
echo
echo "Primeras líneas:"
head "$REPORT"
