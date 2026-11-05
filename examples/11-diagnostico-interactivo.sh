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
    IFS= read -rsn1 _
    printf '\n'
}

red() {
    titulo "INFORMACION DE RED"
    local iface="" ip="" mac="" line
    local encontrados=0

    while IFS= read -r line; do
        line=${line%$'\r'}
        if [[ "$line" == *: && "$line" != [[:space:]]* ]]; then
            if [[ -n "$iface" && ( -n "$ip" || -n "$mac" ) ]]; then
                printf '%s%s%s\n' "$GREEN" "$iface" "$RESET"
                printf '  IPv4 : %s\n  MAC  : %s\n\n' "${ip:--}" "${mac:--}"
                ((encontrados+=1))
            fi
            iface=${line%:}
            ip=""
            mac=""
            continue
        fi
        if [[ "$line" == *IPv4* && "$line" =~ ([0-9]{1,3}\.){3}[0-9]{1,3} ]]; then
            ip="${BASH_REMATCH[0]}"
        fi
        if [[ "$line" =~ ([0-9A-Fa-f]{2}-){5}[0-9A-Fa-f]{2} ]]; then
            mac="${BASH_REMATCH[0]}"
        fi
    done < <(ipconfig.exe /all)

    if [[ -n "$iface" && ( -n "$ip" || -n "$mac" ) ]]; then
        printf '%s%s%s\n' "$GREEN" "$iface" "$RESET"
        printf '  IPv4 : %s\n  MAC  : %s\n\n' "${ip:--}" "${mac:--}"
        ((encontrados+=1))
    fi
    if (( encontrados == 0 )); then
        printf '%sNo se encontraron IP y MAC en ipconfig; adaptadores de SST:%s\n' "$RED" "$RESET"
        net interfaces
    fi
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
IFS= read -r nombre
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
    IFS= read -rsn1 opcion
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
