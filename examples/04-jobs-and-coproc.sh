#!/usr/bin/env nwash
# Concurrencia: jobs Bash, wait -n y coprocesos.

set -o pipefail
TMPROOT="${TEMP:-.}"

probe() {
    local host="$1"
    local output="$2"
    {
        echo "HOST=$host"
        net ping "$host" -c 2
    } > "$output"
}

echo "=== Lanzando tres jobs en background ==="
probe 127.0.0.1 "$TMPROOT/sst-job-local.txt" &
p1=$!
probe 1.1.1.1 "$TMPROOT/sst-job-dns1.txt" &
p2=$!
probe 8.8.8.8 "$TMPROOT/sst-job-dns2.txt" &
p3=$!

printf "PID jobs: %s %s %s\n" "$p1" "$p2" "$p3"
jobs -l

echo
echo "=== Esperando el primero que termine ==="
wait -n -p FINISHED
printf "Primer PID terminado: %s\n" "${FINISHED:-desconocido}"

echo
echo "=== Esperando el resto ==="
wait

for file in "$TMPROOT/sst-job-local.txt" "$TMPROOT/sst-job-dns1.txt" "$TMPROOT/sst-job-dns2.txt"; do
    echo
    echo "--- $file ---"
    cat "$file"
    rm -f "$file"
done

echo
echo "=== Coproceso ==="
coproc TICKER {
    for ((i=1; i<=5; i++)); do
        printf "tick %d @ %s\n" "$i" "$(date)"
        sleep 1
    done
}

printf "TICKER_PID=%s read_fd=%s write_fd=%s\n"     "$TICKER_PID" "${TICKER[0]}" "${TICKER[1]}"

while read -u "${TICKER[0]}" line; do
    printf "coproc -> %s\n" "$line"
done

wait "$TICKER_PID"
echo "Coproceso finalizado."
