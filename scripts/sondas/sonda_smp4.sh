#!/bin/bash
# T-025: sonda de estrés SMP con 4 vCPU (fuera de la suite, que usa 2).
# Exige: 4 LAPIC en la MADT, 3 AP en línea, estrés concurrente OK dos veces, 0 IPI sin respuesta.
cd "$(dirname "$0")/../.." || exit 2
# No matar otras máquinas ni borrar sus discos/sockets: el runner usa copia privada.
PORTIX_SMP=4 SONDA_TIMEOUT=90 timeout 400 python3 scripts/sondas/sonda_secuencia.py \
  "cat /proc/smp_estres" "cat /proc/smp_estres" "cat /proc/smp" > /tmp/sonda_smp4.log 2>&1
fallos=0
comprobar() { if grep -aq -- "$1" /tmp/ext2.log; then echo "  [PASS] $2"; else echo "  [FALLO] $2"; fallos=$((fallos+1)); fi; }
comprobar "nucleos_madt: 4" "MADT con 4 LAPIC"
comprobar "nucleos_en_linea: 4" "4 nucleos en linea"
[ "$(grep -ac '^resultado: OK' /tmp/ext2.log)" = "2" ] && echo "  [PASS] estres concurrente OK x2" || { echo "  [FALLO] estres concurrente OK x2"; fallos=$((fallos+1)); }
comprobar "contador: 16000 esperado: 16000" "contador exacto con 4 nucleos"
if grep -aq "FALLO\|SIN ACUSE\|sin_respuesta: [1-9]" /tmp/ext2.log; then echo "  [FALLO] hay FALLO/SIN ACUSE en la salida"; fallos=$((fallos+1)); else echo "  [PASS] sin FALLO ni SIN ACUSE"; fi
echo "SONDA SMP4: fallos=$fallos"; exit $fallos
