# [T-001] Guion ejecutado por busybox sh (estatico, sin modificar) contra la
# raiz ext2 real de Portix. Cada linea imprime una cifra que la suite exige y
# que ademas el anfitrion comprueba con debugfs/e2fsck sobre la imagen.
B=/bin/busybox
$B rm -rf /var/t001; $B mkdir -p /var/t001 || exit 1
# 1) enlace duro real: nlink=2, mismo inodo, el contenido se ve por ambos nombres
echo "contenido-t001" > /var/t001/a
$B ln /var/t001/a /var/t001/b || echo "T001 ln FALLO"
echo "T001 nlink: $($B stat -c %h /var/t001/a) $($B stat -c %h /var/t001/b)"
echo "T001 inodo igual: $([ "$($B stat -c %i /var/t001/a)" = "$($B stat -c %i /var/t001/b)" ] && echo si || echo no)"
echo "T001 por b: $($B cat /var/t001/b)"
# 2) ln sobre un directorio debe fallar con EPERM; entre sistemas con EXDEV
$B ln /var/t001 /var/t001/d 2>&1 | $B grep -q -i "not permitted\|operation" && echo "T001 ln dir: EPERM"
$B ln /var/t001/a /tmp/t001-x 2>&1 | $B grep -q -i "cross-device\|Invalid cross" && echo "T001 ln exdev: EXDEV"
# 3) chown real (persistido en el inodo: lo comprueba debugfs en el anfitrion)
$B chown 1234:5678 /var/t001/a && echo "T001 chown: $($B stat -c %u:%g /var/t001/a)"
# 4) fichero disperso que obliga al indirecto TRIPLE (bloque 4 KiB: > 4 GiB)
#    4 bloques asignados (tind, dind, ind, dato) aunque el tamano sea 4,5 GiB.
$B rm -f /var/t001/disperso
printf 'TRIPLE' | $B dd of=/var/t001/disperso bs=1 seek=4831838208 conv=notrunc 2>/dev/null
echo "T001 disperso tam: $($B stat -c %s /var/t001/disperso) bloques512: $($B stat -c %b /var/t001/disperso)"
echo "T001 disperso lee: $($B dd if=/var/t001/disperso bs=1 skip=4831838208 count=6 2>/dev/null)"
echo "T001 hueco: $($B dd if=/var/t001/disperso bs=1 skip=4831838000 count=8 2>/dev/null | $B wc -c) bytes, no nulos: $($B dd if=/var/t001/disperso bs=1 skip=4831838000 count=8 2>/dev/null | $B tr -d '\000' | $B wc -c)"
# 5) unlink de un nombre deja el otro con nlink=1 y el dato intacto
$B rm /var/t001/a && echo "T001 tras rm: nlink $($B stat -c %h /var/t001/b) contenido $($B cat /var/t001/b)"
# 6) truncar el disperso a 0 libera el arbol triple (bloques512 vuelve a 0)
$B truncate -s 0 /var/t001/disperso 2>/dev/null || : > /var/t001/disperso
echo "T001 truncado bloques512: $($B stat -c %b /var/t001/disperso)"
# vuelve a crearlo para que el anfitrion vea el TIND con debugfs
printf 'TRIPLE' | $B dd of=/var/t001/disperso bs=1 seek=4831838208 conv=notrunc 2>/dev/null
# 7) FIFO persistente en ext2: inodo S_IFIFO real (debugfs del anfitrion lo ve
#    como "Type: FIFO"); open la enruta a la tuberia con nombre y los datos
#    pasan de un proceso a otro sin tocar el disco.
$B rm -f /var/t001/fifo /var/t001/fifo.out
$B mkfifo /var/t001/fifo && echo "T001 fifo tipo: $($B stat -c %F /var/t001/fifo) tam $($B stat -c %s /var/t001/fifo)"
$B sh -c "$B cat /var/t001/fifo > /var/t001/fifo.out" &
echo "por-la-fifo" > /var/t001/fifo
wait
echo "T001 fifo lee: $($B cat /var/t001/fifo.out)"
$B mkfifo /var/t001/fifo 2>&1 | $B grep -q -i "exists" && echo "T001 fifo repetida: EEXIST"
$B sync
echo "[T001-PASS] ext2: enlaces duros, chown, triple indirecto disperso, fsync, FIFO"
