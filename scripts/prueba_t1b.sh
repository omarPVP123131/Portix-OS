# [CICLO 50 / T1b] Guion ejecutado por `busybox sh` (ash) DINAMICO de Alpine:
# PIE + PT_INTERP, cargado por el ld-musl real de la distro (ambos sin
# modificar). Cada execve de este guion vuelve a pasar por el cargador
# dinamico: si PT_INTERP, mmap(fd,offset), MAP_FIXED o el auxv fallan, las
# cifras no salen y la suite FALLA.
B=/bin/busybox-dyn
echo "T1b pipeline: $(echo hola | $B tr a-z A-Z)"
$B rm -rf /tmp/t1bd; $B mkdir -p /tmp/t1bd && cd /tmp/t1bd && $B touch a b c d e f g && echo "T1b ls|wc: $($B ls | $B wc -l)"
x=5; echo "T1b expr: $($B expr $x + 2)"
for i in 1 2 3; do printf "%s" "$i"; done; echo " T1b bucle"
echo "T1b fork+exec: $($B sh -c 'echo anidado')"
$B md5sum /etc/hostname | $B cut -c1-32 > /tmp/t1b.md5 && echo "T1b redireccion: $($B cat /tmp/t1b.md5 | $B wc -c) bytes"
# El interprete real como programa: ld-musl carga y arranca a busybox el mismo
# (mmap con offset + MAP_FIXED desde espacio de usuario) y el hermano de la
# tuberia sobrevive a su salida (FS_BASE restaurado en la conmutacion por exit).
echo "T1b ldso: $(/lib/ld-musl-x86_64.so.1 $B echo via-ldso | $B tr a-z A-Z)"
echo "[T1B-PASS] busybox musl DINAMICO (PIE + ld-musl de Alpine): pipes, subshells, bucles, redireccion"
