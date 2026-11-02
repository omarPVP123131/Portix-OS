# [T-004] glibc REAL de Debian trixie (libc6 2.41 + libgcc-s1 + gcc-14-base) y el
# paquete `hello` (GNU Hello, ELF dinamico enlazado contra glibc), todos .deb sin
# modificar instalados con `pkg deb-add hello`. Arranca via /lib64/ld-linux-x86-64.so.2
# y usa /usr/lib/x86_64-linux-gnu/libc.so.6 dentro de Portix.
B=/bin/busybox
echo "T004 ld-linux: $($B stat -L -c %s /lib64/ld-linux-x86-64.so.2) bytes libc.so.6: $($B stat -c %s /usr/lib/x86_64-linux-gnu/libc.so.6) bytes"
echo "T004 /lib64 -> $($B readlink /lib64)"
/usr/lib/x86_64-linux-gnu/libc.so.6 2>&1 | $B head -1 | $B sed 's/^/T004 libc: /'
/usr/bin/hello || { echo "T004 hello FALLO"; exit 1; }
/usr/bin/hello --greeting="T004 saludo desde glibc en Portix" || exit 1
# Debian imprime "hello (GNU Hello) " + "Copyright (C) 2025 Free Software Foundation, Inc." (mismo texto que en un Linux anfitrion)
/usr/bin/hello --version | $B grep -c "GNU Hello\|Free Software Foundation" | $B sed 's/^/T004 lineas de --version: /'
echo "T004 pila: $(/usr/bin/hello --greeting="$($B head -c 100000 /dev/zero | $B tr '\0' x)" | $B wc -c) bytes (argumento de 100000 bytes via pila extendida)"
echo "[T004-PASS] glibc de Debian (ld-linux + libc.so.6) ejecuta un .deb de la distro dentro de Portix"
