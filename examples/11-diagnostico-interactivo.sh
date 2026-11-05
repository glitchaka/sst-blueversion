#!/usr/bin/env bash
# Diagnóstico interactivo SST/Nwash. Solo lecturas del sistema.
RESET=$'\033[0m'
CYAN=$'\033[1;36m'
GREEN=$'\033[1;32m'
MAGENTA=$'\033[1;35m'
YELLOW=$'\033[1;33m'
RED=$'\033[1;31m'

panel() {
    local msg="$1"
    local ancho=$(( ${#msg} + 4 ))
    local borde
    printf -v borde '%*s' "$ancho" ''
    borde=${borde// /═}
    printf '\n%s╔%s╗\n' "$CYAN" "$borde"
    printf '║  %s%s%s  %s║\n' "$MAGENTA" "$msg" "$CYAN" "$CYAN"
    printf '╚%s╝%s\n\n' "$borde" "$RESET"
}

titulo() {
    printf '\n%s═══ %s ═══%s\n\n' "$CYAN" "$1" "$RESET"
}

pausa() {
    printf '\n%sPresiona una tecla para volver...%s' "$YELLOW" "$RESET"
    if ! IFS= read -rsn1 _; then
        printf '\n'
        return 1
    fi
    printf '\n'
}

red() {
    titulo "INFORMACION DE RED"
    # SST consulta las interfaces por IP Helper API de Windows (IPv4, MAC,
    # enlace y adaptadores). No depende de la configuración regional de
    # ipconfig.exe ni de interpretar su salida mediante expresiones regulares.
    net interfaces
}

equipo() {
    titulo "IDENTIFICACION DEL EQUIPO"
    printf '%sNombre del equipo:%s %s\n' "$GREEN" "$RESET" "$(sys hostname)"
    printf '%sUsuario actual:%s %s\n' "$GREEN" "$RESET" "$(sys whoami)"
}

procesos() {
    titulo "PROCESOS CON MAYOR CONSUMO DE RAM"
    sys processes --sort ram --top 8
}

impresoras() {
    titulo "IMPRESORAS INSTALADAS"
    sys printers
}

printf '%s¿Cómo te llamas? %s' "$CYAN" "$RESET"
if ! IFS= read -r nombre; then
    printf '\nNo se recibió el nombre. Fin de entrada.\n'
    exit 1
fi
[[ -n "$nombre" ]] || nombre="Invitado"
panel "¡Hola, $nombre! Bienvenido a SST"

while true; do
    printf '%s¿Qué quieres saber?%s\n\n' "$GREEN" "$RESET"
    printf '%s[1]%s Dirección IP y MAC\n' "$CYAN" "$RESET"
    printf '%s[2]%s Nombre del equipo y usuario\n' "$CYAN" "$RESET"
    printf '%s[3]%s Procesos con mayor consumo\n' "$CYAN" "$RESET"
    printf '%s[4]%s Impresoras instaladas e IP\n' "$CYAN" "$RESET"
    printf '%s[0]%s Salir\n\n' "$RED" "$RESET"
    printf '%sSelecciona una opción: %s' "$YELLOW" "$RESET"
    if ! IFS= read -rsn1 opcion; then
        printf '\nFin de entrada.\n'
        break
    fi
    printf '\n'
    case "$opcion" in
        1) red; pausa ;;
        2) equipo; pausa ;;
        3) procesos; pausa ;;
        4) impresoras; pausa ;;
        0) printf '%sHasta pronto, %s.%s\n' "$MAGENTA" "$nombre" "$RESET"; break ;;
        *) printf '%sOpción no válida.%s\n' "$RED" "$RESET" ;;
    esac
    printf '\n'
done
