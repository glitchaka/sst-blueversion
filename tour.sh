# SST Tour — selector interactivo de ejemplos.
# También puede lanzarse escribiendo simplemente: tour

while true; do
    selected="$(sst-tour-select)"
    status=$?

    if (( status != 0 )) || [[ -z "$selected" ]]; then
        break
    fi

    if [[ ! -f "$selected" ]]; then
        clear
        echo "SST TOUR"
        echo "================================================================"
        echo "ERROR: el selector devolvió una ruta que no existe:"
        echo "$selected"
        echo "================================================================"
        echo "Pulsa Enter para volver al tour"
        read -r
        continue
    fi

    clear
    echo "SST TOUR"
    echo "================================================================"
    echo "Ejecutando: $(basename "$selected")"
    echo "Ruta: $selected"
    echo "================================================================"
    echo

    "$selected"
    script_status=$?

    echo
    echo "================================================================"
    printf "Ejemplo terminado · status=%s\n" "$script_status"
    echo "Pulsa Enter para volver al tour · q + Enter para salir"
    read -r answer
    if [[ "$answer" == "q" || "$answer" == "Q" ]]; then
        break
    fi
done

clear
echo "SST Tour finalizado."
