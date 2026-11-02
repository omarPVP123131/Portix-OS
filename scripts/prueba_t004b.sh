# [T-004b] Cadena de confianza del indice de Debian: InRelease firmado (OpenPGP RSA-4096,
# clave del archivo 13/trixie en /etc/pkg/deb-keys) -> SHA-256 de Packages.gz -> SHA-256 de
# cada .deb. Un InRelease con UN byte cambiado en el cuerpo firmado debe ser RECHAZADO.
B=/bin/busybox
R=/var/pkg/deb/trixie_InRelease
pkg deb-verify $R || { echo "T004B el InRelease real NO verifica"; exit 1; }
$B sed 's/^Origin: Debian$/Origin: Debiam/' $R > /tmp/InRelease.mal
$B cmp -s $R /tmp/InRelease.mal && { echo "T004B la manipulacion no cambio nada"; exit 1; }
if pkg deb-verify /tmp/InRelease.mal; then echo "T004B FALLO: un InRelease manipulado fue aceptado"; exit 1; fi
echo "[T004B-PASS] InRelease real aceptado y InRelease manipulado rechazado (firma OpenPGP verificada de verdad)"
