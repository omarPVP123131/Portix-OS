# [T-015] El paso FIFO de T-001, repetido: es el que se colgó una vez en la
# pasada 72a (se detuvo tras `T001 fifo tipo:` y no imprimió `T001 fifo lee:`
# en 90 s). El patrón es: `cat fifo > out &`, escritura en la FIFO y `wait`.
# Aquí se repite N veces seguidas en el MISMO proceso del shell, de modo que un
# cuelgue de `wait`, de la lectura de la FIFO o del cierre del escritor aparece
# en la primera vuelta que no termina, con su número.
#
# Cada vuelta escribe una marca `[T015-FIFO] i=<n>` antes de empezar; si la
# sonda deja de ver marcas, el cuelgue está en esa vuelta y no en otra.
B=/bin/busybox
N=${1:-20}
$B rm -rf /var/t015; $B mkdir -p /var/t015 || exit 1
i=0
while [ "$i" -lt "$N" ]; do
    i=$((i + 1))
    echo "[T015-FIFO] i=$i inicia"
    $B rm -f /var/t015/fifo /var/t015/fifo.out
    $B mkfifo /var/t015/fifo || echo "[T015-FIFO] i=$i mkfifo FALLO"
    $B sh -c "$B cat /var/t015/fifo > /var/t015/fifo.out" &
    echo "por-la-fifo" > /var/t015/fifo
    wait
    leido=$($B cat /var/t015/fifo.out)
    if [ "$leido" = "por-la-fifo" ]; then
        echo "[T015-FIFO] i=$i ok leido=$leido"
    else
        echo "[T015-FIFO] i=$i DATO DISTINTO leido='$leido'"
    fi
done
echo "[T015-FIFO-STATS] vueltas=$i"
echo "[T015-FIFO-PASS] el paso FIFO de T-001 completo $i veces seguidas"
