# Panel operativo no destructivo construido con funciones Nwash.
# En el tour se ejecuta de forma secuencial para que toda la salida quede visible.

show_system() {
    echo "=== SISTEMA ==="
    sys fetch --small
    echo
    sys uptime
    sys memory
}

show_network() {
    echo
    echo "=== RED ==="
    net interfaces
    echo
    net neighbors
}

show_devices() {
    echo
    echo "=== INVENTARIO ==="
    device list
    echo
    echo "No inventariados:"
    device unknown
}

show_security() {
    echo
    echo "=== SEGURIDAD ==="
    triage
    echo
    sys suspicious
}

echo "Shell Shock Tool — Operator Console"
show_system
show_network
show_devices
show_security

echo
echo "=== FONDOS ==="
config bg

echo
echo "Operator Console finalizada."
