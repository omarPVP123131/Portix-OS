#!/bin/busybox sh
B=/bin/busybox
$B mkdir -p /var/caza
i=0
while [ $i -lt 100 ]; do
  i=$((i+1))
  $B rm -f /var/caza/fifo /var/caza/out
  $B mkfifo /var/caza/fifo || { echo "CAZA $i mkfifo FALLO"; exit 1; }
  $B sh -c "$B cat /var/caza/fifo > /var/caza/out" &
  echo "dato-$i" > /var/caza/fifo
  wait
  r=$($B cat /var/caza/out)
  [ "$r" = "dato-$i" ] || { echo "CAZA $i FALLO: '$r'"; exit 1; }
  echo "CAZA $i ok $($B date +%s)"
done
echo "CAZA-FIN"
