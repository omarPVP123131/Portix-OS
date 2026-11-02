# [T-002] Guion de busybox sh: tras `pkg add tcc tcc-libs-static musl-dev`, los
# enlaces simbolicos que trae musl-dev deben SEGUIRSE (open/stat), y tcc debe
# enlazar contra ellos. Cada linea puede fallar; el PASS final es condicional.
B=/bin/busybox
echo "T002 libc.so readlink: $($B readlink /usr/lib/libc.so)"
echo "T002 libc.so tipo lstat: $($B stat -c %F /usr/lib/libc.so)"
echo "T002 libc.so tipo stat: $($B stat -L -c %F /usr/lib/libc.so)"
echo "T002 libc.so por el enlace: $($B head -c 4 /usr/lib/libc.so | $B wc -c) bytes, ELF: $($B head -c 4 /usr/lib/libc.so | $B grep -c ELF)"
echo "T002 ld-musl por el enlace de lib: $($B stat -L -c %s /lib/libc.musl-x86_64.so.1) = $($B stat -c %s /lib/ld-musl-x86_64.so.1)"
$B rm -f /tmp/prueba_apk
/usr/bin/tcc -o /tmp/prueba_apk /usr/src/prueba_tcc.c || exit 1
echo "T002 tcc enlazo: $($B stat -c %s /tmp/prueba_apk) bytes"
/tmp/prueba_apk z || exit 1
echo "[T002-PASS] enlaces simbolicos de Alpine seguidos y tcc enlaza contra libc.so"
