#!/usr/bin/env nwash
# Menú interactivo construido completamente con sintaxis Bash/Nwash.

show_system() {
    clear
    sys fetch --small
    echo
    sys uptime
    sys memory
}

show_network() {
    clear
    net interfaces
    echo
    net neighbors
}

show_devices() {
    clear
    device list
    echo
    echo "No inventariados:"
    device unknown
}

show_security() {
    clear
    triage
    echo
    sys suspicious
}

PS3="SST> "
options=(
    "Sistema"
    "Red"
    "Inventario"
    "Seguridad"
    "Fondos"
    "Salir"
)

echo "Shell Shock Tool — Operator Console"

select option in "${options[@]}"; do
    case "$option" in
        Sistema)
            show_system
            ;;
        Red)
            show_network
            ;;
        Inventario)
            show_devices
            ;;
        Seguridad)
            show_security
            ;;
        Fondos)
            clear
            config bg
            ;;
        Salir)
            break
            ;;
        *)
            echo "Opción inválida"
            ;;
    esac
    echo
    echo "Pulsa Enter para continuar..."
    read
    clear
done

echo "Sesión de operador finalizada."
