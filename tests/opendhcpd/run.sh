#!/bin/sh
# Roda dentro de `unshare -r -n`: interface falsa, servidor, pacotes de teste e a página HTTP.
# Uso: run.sh <binário opendhcpd> <pasta de trabalho>
BIN="$1"; W="$2"; T="$(dirname "$0")"
rm -rf "$W"; mkdir -p "$W/log"
cat > "$W/opendhcp.ini" <<EOF
[LISTEN_ON]
192.168.50.1

[LOGGING]
LogLevel=All

[RANGE_SET]
DHCPRange=192.168.50.100-192.168.50.200
SubnetMask=255.255.255.0

[GLOBAL_OPTIONS]
AddressTime=3600
EOF
ip link add d0 type dummy && ip addr add 192.168.50.1/24 dev d0 && ip link set d0 up && ip link set lo up
"$BIN" -v -i "$W/opendhcp.ini" -s "$W/opendhcp.state" -l "$W/log/opendhcp%Y%m%d.log" > "$W/stdout.txt" 2>&1 &
PID=$!
python3 -c "import time; time.sleep(2)"
python3 "$T/send.py" 192.168.50.1
# página de status (padrão novo: só 127.0.0.1) com caminho enorme
python3 - <<'PY'
import socket
for path in ["/", "/" + "A" * 1500]:
    try:
        c = socket.create_connection(("127.0.0.1", 6789), timeout=3)
        c.sendall(f"GET {path} HTTP/1.1\r\nHost: x\r\n\r\n".encode())
        data = b""
        while True:
            chunk = c.recv(65536)
            if not chunk:
                break
            data += chunk
        print(f"HTTP {path[:12]}...: {len(data)} bytes; script na página: {b'<script>' in data}")
    except Exception as e:
        print(f"HTTP {path[:12]}...: erro {e}")
PY
python3 -c "import time; time.sleep(1)"
if kill -0 $PID 2>/dev/null; then echo "SERVIDOR: vivo"; kill $PID; else echo "SERVIDOR: CAIU"; wait $PID; echo "status=$?"; fi
echo "--- stdout (linhas com Host/evil/fake)"
grep -a -n "Host \|evil\|fake\|Offer\|offered" "$W/stdout.txt" | head -12
