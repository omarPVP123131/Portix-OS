/* lib/examples/curl.c — Lightweight HTTP / Network fetch client */
#include <portix.h>
#include <string.h>

static int parse_int(const char *s) {
    int n = 0;
    while (*s >= '0' && *s <= '9') {
        n = n * 10 + (*s - '0');
        s++;
    }
    return n;
}

int main(int argc, char **argv) {
    const char *host = (argc >= 2) ? argv[1] : "10.0.2.2";
    int port = (argc >= 3) ? parse_int(argv[2]) : 80;
    const char *path = (argc >= 4) ? argv[3] : "/";

    if (argc < 2) {
        printf("Sin argumentos. Usando destino por defecto: http://%s:%d%s\n", host, port, path);
        printf("(Uso: curl <ip_o_host> [puerto] [ruta])\n");
    }

    printf("[CURL] Solicitando http://%s:%d%s ...\n", host, port, path);

    int sock = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock < 0) {
        printf("curl: error al crear socket\n");
        return 1;
    }

    struct sockaddr_in dest;
    memset(&dest, 0, sizeof(dest));
    dest.sin_family = AF_INET;
    dest.sin_port = ((port & 0xFF) << 8) | ((port >> 8) & 0xFF);

    unsigned char ip[4] = {10, 0, 2, 2}; // default gateway / router
    // Parse IPv4 dot-decimal
    int octets[4] = {0, 0, 0, 0};
    int oct_idx = 0;
    int cur = 0;
    int is_ip = 1;
    for (int i = 0; host[i]; i++) {
        if (host[i] >= '0' && host[i] <= '9') {
            cur = cur * 10 + (host[i] - '0');
        } else if (host[i] == '.') {
            if (oct_idx < 3) {
                octets[oct_idx++] = cur;
                cur = 0;
            } else { is_ip = 0; break; }
        } else {
            is_ip = 0;
            break;
        }
    }
    if (is_ip && oct_idx == 3) {
        octets[3] = cur;
        for (int i = 0; i < 4; i++) ip[i] = (unsigned char)octets[i];
    }
    memcpy(&dest.sin_addr, ip, 4);

    char req[256];
    snprintf(req, sizeof(req),
             "GET %s HTTP/1.0\r\nHost: %s\r\nUser-Agent: PortixOS-curl/1.0\r\n\r\n",
             path, host);

    ssize_t sent = sendto(sock, req, strlen(req), 0, (struct sockaddr*)&dest, sizeof(dest));
    if (sent < 0) {
        printf("curl: error al enviar peticion HTTP\n");
        close(sock);
        return 1;
    }

    printf("[CURL] %d bytes enviados. Esperando respuesta...\n", (int)sent);

    char buf[1024];
    memset(buf, 0, sizeof(buf));
    int received = 0;

    for (int t = 0; t < 50; t++) {
        ssize_t n = recvfrom(sock, buf, sizeof(buf) - 1, 0, NULL, NULL);
        if (n > 0) {
            buf[n] = '\0';
            printf("\n--- RESPUESTA HTTP RECIBIDA ---\n%s\n-------------------------------\n", buf);
            received = 1;
            break;
        }
        sleep(1);
    }

    if (!received) {
        printf("[CURL] Peticion HTTP enviada a %d.%d.%d.%d:%d (OK)\n",
               ip[0], ip[1], ip[2], ip[3], port);
    }

    close(sock);
    return 0;
}
