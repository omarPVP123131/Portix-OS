#!/bin/busybox sh
# [b192 / T-013] FHS minimo presente en la raiz ext2 y open(O_CREAT)+exec+unlink en /usr/bin.
# Antes /usr/bin no existia en la raiz y cualquier open(O_CREAT) alli devolvia ENOENT.
for d in /usr/bin /usr/sbin /sbin /opt /srv /run /var/tmp /var/log; do
    [ -d "$d" ] || { echo "T013-FALTA $d"; exit 1; }
done
printf '#!/bin/busybox sh\necho T013-EJECUTADO $1\n' > /usr/bin/t013prueba || { echo "T013-FALLO open(O_CREAT) /usr/bin errno=$?"; exit 1; }
/bin/busybox chmod 755 /usr/bin/t013prueba || { echo "T013-FALLO chmod"; exit 1; }
PATH=/bin:/usr/bin t013prueba hola || { echo "T013-FALLO exec via PATH"; exit 1; }
/bin/busybox rm /usr/bin/t013prueba || { echo "T013-FALLO unlink"; exit 1; }
[ ! -e /usr/bin/t013prueba ] || { echo "T013-FALLO sigue existiendo"; exit 1; }
echo "T013-PASS"
