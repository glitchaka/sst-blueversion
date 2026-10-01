# Tour del lenguaje Nwash/Bash dentro de SST.

set -o pipefail

banner() {
    printf "\n=== %s ===\n" "$1"
}

banner "Identidad de la shell"
printf "NWASH_VERSION=%s\n" "$NWASH_VERSION"
printf "NWASH_BASH_BASE=%s\n" "$NWASH_BASH_BASE"
printf "NWASH_PLATFORM=%s\n" "$NWASH_PLATFORM"
printf "BASH_VERSION=%s\n" "$BASH_VERSION"

banner "Arrays indexados"
targets=("localhost" "127.0.0.1" "example.com")
for ((i=0; i<${#targets[@]}; i++)); do
    printf "[%d] %s\n" "$i" "${targets[$i]}"
done

banner "Array asociativo"
declare -A capability
capability[language]="Bash-compatible"
capability[platform]="Windows"
capability[editor]="Helix-SST"
capability[network]="native"

for key in language platform editor network; do
    printf "%-10s -> %s\n" "$key" "${capability[$key]}"
done

banner "Funciones + local + aritmética"
score() {
    local base="$1"
    local bonus="${2:-0}"
    local total=$((base + bonus))
    printf "%d" "$total"
}

for value in 2 4 8; do
    result="$(score "$value" 3)"
    if (( result >= 10 )); then
        printf "%s -> alto\n" "$result"
    elif (( result >= 6 )); then
        printf "%s -> medio\n" "$result"
    else
        printf "%s -> bajo\n" "$result"
    fi
done

banner "case"
mode="${1:-demo}"
case "$mode" in
    demo|showcase)
        echo "modo de demostración"
        ;;
    quiet)
        echo "modo silencioso"
        ;;
    *)
        echo "modo personalizado: $mode"
        ;;
esac

banner "[[ ... ]]"
sample="informe.txt"
if [[ "$sample" == *.txt && -n "$sample" ]]; then
    echo "$sample parece un archivo de texto"
fi

banner "Here-document"
cat <<EOF
SST puede ejecutar scripts .sh dentro de su propio intérprete.
No necesita lanzar Bash externo.
Directorio actual: $PWD
Usuario: ${USERNAME:-desconocido}
EOF

banner "Expansiones"
name="${2:-shell-shock-tool}"
printf "valor       : %s\n" "$name"
printf "por defecto : %s\n" "${UNDEFINED_VALUE:-fallback}"
printf "longitud    : %s\n" "${#name}"

banner "Tiempo de un comando"
time sys uptime

echo
echo "Fin de 01-language-tour.sh"
