# Mini ETL con las utilidades Unix integradas en SST.

set -o pipefail

TMPROOT="${TEMP:-.}"
DATA="$TMPROOT/sst-demo-data-$$.csv"
NORMALIZED="$TMPROOT/sst-demo-normalized-$$.csv"

cleanup() {
    rm -f "$DATA" "$NORMALIZED"
}
trap cleanup EXIT

cat > "$DATA" <<'EOF'
host,area,state
PC-04,biblioteca,online
PC-01,recepcion,online
PC-03,biblioteca,offline
PC-02,soporte,online
PC-05,biblioteca,online
PC-02,soporte,online
EOF

echo "=== Dataset ==="
cat "$DATA"

echo
echo "=== Líneas online ==="
grep online "$DATA"

echo
echo "=== Áreas, ordenadas y únicas ==="
cut -d , -f 2 "$DATA" | sort | uniq

echo
echo "=== Normalización con sed ==="
sed "s/online/UP/g" "$DATA" > "$NORMALIZED"
cat "$NORMALIZED"

echo
echo "=== Conteo ==="
wc "$DATA"

echo
echo "=== Integridad ==="
sha256sum "$DATA"

echo
echo "=== Tee + filtro ==="
grep biblioteca "$DATA" | tee "$TMPROOT/sst-biblioteca-$$.txt" | sort
rm -f "$TMPROOT/sst-biblioteca-$$.txt"

echo
echo "=== Base64 ida/vuelta ==="
base64 "$DATA" > "$TMPROOT/sst-data-$$.b64"
base64 -d "$TMPROOT/sst-data-$$.b64" > "$TMPROOT/sst-data-$$.decoded"
diff "$DATA" "$TMPROOT/sst-data-$$.decoded"
rm -f "$TMPROOT/sst-data-$$.b64" "$TMPROOT/sst-data-$$.decoded"

echo
echo "Pipeline completado."
