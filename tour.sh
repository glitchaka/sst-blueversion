#!/usr/bin/env bash
# SST Tour — selector interactivo integrado, sin comandos externos ocultos.
# El contenido de este archivo se instala solo si tour.sh no existe.

while true; do
    printf '\nSST TOUR — Ejemplos de Nwash\n'
    printf '============================================\n'
    printf '  1) Lenguaje Bash / Nwash\n'
    printf '  2) Informe de operador Windows\n'
    printf '  3) Descubrimiento de red\n'
    printf '  4) Trabajos y coprocesos\n'
    printf '  5) Tuberías y texto\n'
    printf '  6) Análisis de seguridad\n'
    printf '  7) Consola del operador\n'
    printf '  8) Configuración y fondos\n'
    printf '  9) Bash avanzado\n'
    printf ' 10) Demostración completa\n'
    printf '  0) Salir\n'
    printf '============================================\n'
    printf 'Selecciona un número y pulsa Enter: '

    if IFS= read -r opcion; then
        :
    else
        printf '\nFin de la entrada.\n'
        break
    fi

    selected=''
    case "$opcion" in
        1)  selected='examples/01-language-tour.sh' ;;
        2)  selected='examples/02-windows-operator-report.sh' ;;
        3)  selected='examples/03-network-discovery.sh' ;;
        4)  selected='examples/04-jobs-and-coproc.sh' ;;
        5)  selected='examples/05-pipelines-and-text.sh' ;;
        6)  selected='examples/06-security-triage.sh' ;;
        7)  selected='examples/07-operator-console.sh' ;;
        8)  selected='examples/08-config-and-backgrounds.sh' ;;
        9)  selected='examples/09-advanced-bash.sh' ;;
        10) selected='examples/10-full-showcase.sh' ;;
        0|q|Q) break ;;
        *) printf 'Opción no válida: %s\n' "$opcion"; continue ;;
    esac

    if [[ ! -f "$selected" ]]; then
        printf 'No se encontró: %s\n' "$selected"
        continue
    fi

    printf '\nEjecutando: %s\n' "$selected"
    printf '============================================\n'
    "$selected"
    script_status=$?
    printf '\n============================================\n'
    printf 'Ejemplo terminado. Estado: %s\n' "$script_status"
    printf 'Pulsa Enter para regresar al menú...'
    if IFS= read -r pausa; then
        :
    else
        break
    fi
done

printf '\nSST Tour finalizado.\n'
