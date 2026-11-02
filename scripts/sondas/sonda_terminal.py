# Sonda rapida: arranca la imagen en QEMU headless, envia ordenes al terminal
# del nucleo y guarda el log en /tmp/ext2.log. Uso:
#   python3 scripts/sondas/sonda_terminal.py "ls /" "cat /etc/passwd"
import sys, time
sys.path.insert(0, 'scripts')
from headless_runner import PortixHeadlessRunner
r = PortixHeadlessRunner('build/dist/portix.img', timeout=30, verbose=False)
r.start_http_server(8000); r.start_https_server(port=8443)
r.start_qemu()
ok,_ = r.wait_for_pattern("present done", 0, 60)
for c in sys.argv[1:]:
    r.send_command(c); time.sleep(float(__import__("os").environ.get("SONDA_ESPERA","8")))
open('/tmp/ext2.log','w').write(r.log)
print("boot", ok)
r.close()
