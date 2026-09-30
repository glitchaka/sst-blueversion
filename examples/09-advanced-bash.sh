#!/usr/bin/env nwash
# Características Bash avanzadas implementadas por Nwash.

set -o pipefail

usage() {
    echo "uso: $0 [-n NOMBRE] [-c CANTIDAD]"
}

name="Nwash"
count=4

while getopts "n:c:h" opt; do
    case "$opt" in
        n) name="$OPTARG" ;;
        c) count="$OPTARG" ;;
        h) usage; exit 0 ;;
        *) usage; exit 2 ;;
    esac
done
shift $((OPTIND - 1))

echo "=== printf -v ==="
printf -v greeting "Hola %s" "$name"
echo "$greeting"

echo
echo "=== Nameref ==="
declare original="valor inicial"
declare -n ref=original
ref="modificado por nameref"
printf "original=%s\n" "$original"

echo
echo "=== Array disperso ==="
sparse=([0]="cero" [3]="tres" [9]="nueve")
for index in 0 3 9; do
    printf "sparse[%d]=%s\n" "$index" "${sparse[$index]}"
done

echo
echo "=== Brace expansion + bucle aritmético ==="
items=(demo-{1..5}.txt)
for ((i=0; i<count && i<${#items[@]}; i++)); do
    printf "%d -> %s\n" "$i" "${items[$i]}"
done

echo
echo "=== mapfile/readarray ==="
TMP="${TEMP:-.}/sst-mapfile-$$.txt"
trap 'rm -f "$TMP"' EXIT

cat > "$TMP" <<'EOF'
alpha
beta
gamma
delta
EOF

mapfile -t rows < "$TMP"
printf "filas=%d primera=%s ultima=%s\n" "${#rows[@]}" "${rows[0]}" "${rows[3]}"

echo
echo "=== Parámetros posicionales + shift ==="
set -- rojo verde azul
printf "argc=%s primero=%s\n" "$#" "$1"
shift
printf "tras shift: argc=%s primero=%s\n" "$#" "$1"

echo
echo "=== Subshell vs grupo ==="
value="padre"
( value="subshell"; printf "dentro subshell: %s\n" "$value" )
printf "fuera subshell : %s\n" "$value"
{ value="grupo"; printf "dentro grupo   : %s\n" "$value"; }
printf "fuera grupo    : %s\n" "$value"

echo
echo "=== Información de llamada ==="
where_am_i() {
    printf "FUNCNAME[0]=%s\n" "${FUNCNAME[0]}"
    printf "BASH_SOURCE[0]=%s\n" "${BASH_SOURCE[0]}"
    printf "BASH_LINENO[0]=%s\n" "${BASH_LINENO[0]}"
    caller 0
}
where_am_i

echo
echo "Fin del showcase Bash avanzado."
