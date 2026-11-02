# [CICLO 49 / T1] Guion ejecutado por `busybox sh` (ash) de Alpine Linux:
# binario musl ESTATICO, no-PIE, sin modificar, corriendo sobre Portix.
# Cada linea la comprueba la suite headless. Si fork/execve/pipe/dup2/wait4
# del traductor Linux fallan, las cifras no salen y la suite FALLA.
B=/bin/busybox
echo "T1 pipeline: $(echo hola | $B tr a-z A-Z)"
# Directorio propio (el estado de /etc depende de pruebas anteriores de la suite).
$B rm -rf /tmp/t1d; $B mkdir -p /tmp/t1d && cd /tmp/t1d && $B touch a b c d e f g && echo "T1 ls|wc: $($B ls | $B wc -l)"
x=5; echo "T1 expr: $($B expr $x + 2)"
$B true && echo "T1 and: ok"
$B false || echo "T1 or: ok"
for i in 1 2 3; do printf "%s" "$i"; done; echo " T1 bucle"
echo "T1 fork+exec: $($B sh -c 'echo anidado')"
$B md5sum /etc/hostname | $B cut -c1-32 > /tmp/t1.md5 && echo "T1 redireccion: $($B cat /tmp/t1.md5 | $B wc -c) bytes"
echo "T1 uname: $($B uname -s -m)"
echo "[T1-PASS] busybox musl estatico: pipes, subshells, bucles, redireccion"
