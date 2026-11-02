# [T-003] gcc REAL de Alpine (cpp -> cc1 -> as -> ld, binarios sin modificar)
# compilando C dentro de Portix contra musl-dev y ejecutando el resultado.
B=/bin/busybox
$B rm -f /tmp/hola_gcc /tmp/hola_gcc.o
echo "T003 gcc: $(/usr/bin/gcc -dumpversion) as: $(/usr/bin/as --version | $B head -1) ld: $(/usr/bin/ld --version | $B head -1)"
/usr/bin/gcc -v -O2 -c -o /tmp/hola_gcc.o /usr/src/prueba_tcc.c 2>&1 | $B grep -E "cc1|as " | $B head -2
echo "T003 objeto: $($B stat -c %s /tmp/hola_gcc.o) bytes"
/usr/bin/gcc -O2 -o /tmp/hola_gcc /tmp/hola_gcc.o || { echo "T003 enlace FALLO"; exit 1; }
echo "T003 enlazado: $($B stat -c %s /tmp/hola_gcc) bytes"
/tmp/hola_gcc a b c || exit 1
/usr/bin/gcc -static -O2 -o /tmp/hola_static /usr/src/prueba_tcc.c && /tmp/hola_static q && echo "T003 estatico OK"
echo "[T003-PASS] gcc de Alpine compilo, enlazo y el ejecutable corrio dentro de Portix"
