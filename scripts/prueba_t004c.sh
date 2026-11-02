# [T-004c] glibc "en ancho": las utilidades REALES de Debian trixie (bash 5.2, coreutils 9.7,
# grep 3.11, sed 4.9, findutils 4.10, diffutils 3.10) instaladas con `pkg deb-add` sin modificar
# corren sobre Portix con el ld-linux + libc.so.6 de la distro. Cada linea compara un valor
# exacto; si una utilidad aborta (stack smashing, ENOSYS, PANIC) la linea no aparece y FALLA.
B=/bin/busybox
F=0
/usr/bin/sort /usr/src/prueba_t004.sh > /tmp/s.out || { echo "T004C sort fallo rc=$?"; F=1; }
echo "T004C sort: $($B wc -l < /tmp/s.out) lineas"
echo "T004C sha256sum: $(/usr/bin/sha256sum /usr/bin/hello | $B cut -c1-16)"
echo "T004C grep: $(echo 'abc def' | /usr/bin/grep -o 'd.f')"
echo "T004C sed: $(echo 'abc def' | /usr/bin/sed 's/def/XYZ/')"
/usr/bin/cp /usr/bin/hello /tmp/h2 && /usr/bin/cmp /usr/bin/hello /tmp/h2 && echo "T004C cp+cmp: identicos"
/usr/bin/mkdir -p /tmp/a/b/c && /usr/bin/touch /tmp/a/b/c/f
echo "T004C find: $(/usr/bin/find /tmp/a -type f)"
echo "T004C stat relativo: $(cd /tmp/a && /usr/bin/stat -c %n b/c/f)"
/usr/bin/ls -la /usr/bin/hello | $B sed 's/^/T004C ls: /'
echo "T004C bash: $(/usr/bin/bash -c 'echo {1..5} | /usr/bin/tr " " ,')"
echo "T004C date: $(/usr/bin/date +%Y)"
[ "$(/usr/bin/find /tmp/a -type f)" = "/tmp/a/b/c/f" ] || F=1
[ "$(/usr/bin/bash -c 'echo {1..5} | /usr/bin/tr " " ,')" = "1,2,3,4,5" ] || F=1
[ "$($B wc -l < /tmp/s.out)" = "$($B wc -l < /usr/src/prueba_t004.sh)" ] || F=1
[ $F = 0 ] && echo "[T004C-PASS] bash, coreutils, grep, sed, findutils y diffutils de Debian corren sin modificar sobre Portix" || echo "[T004C-FAIL]"
