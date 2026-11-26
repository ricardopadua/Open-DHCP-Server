#!/bin/sh
# Ciclo completo com reinício: ACK grava o estado; o servidor reinicia e carrega o estado; o mesmo
# MAC volta com nome de 64 caracteres (caminho do estouro do nome carregado do estado).
BIN="$1"; W="$2"; T="$(dirname "$0")"
rm -rf "$W"; mkdir -p "$W/log"
printf '[LISTEN_ON]\n192.168.50.1\n\n[LOGGING]\nLogLevel=All\n\n[RANGE_SET]\nDHCPRange=192.168.50.100-192.168.50.200\nSubnetMask=255.255.255.0\n\n[GLOBAL_OPTIONS]\nAddressTime=3600\n' > "$W/opendhcp.ini"
ip link add d0 type dummy && ip addr add 192.168.50.1/24 dev d0 && ip link set d0 up && ip link set lo up
start() { "$BIN" -v -i "$W/opendhcp.ini" -s "$W/opendhcp.state" -l "$W/log/opendhcp%Y%m%d.log" >> "$W/stdout.txt" 2>&1 & PID=$!; python3 -c "import time; time.sleep(2)"; }
start
python3 "$T/cycle.py" 192.168.50.1 curto 77
python3 -c "import time; time.sleep(1)"
kill $PID; wait $PID 2>/dev/null
echo "estado: $(stat -c '%s bytes, modo %a' "$W/opendhcp.state")"
start
python3 "$T/cycle.py" 192.168.50.1 "$(python3 -c "print('n'*64)")" 77
python3 -c "import time; time.sleep(1)"
if kill -0 $PID 2>/dev/null; then echo "SERVIDOR: vivo depois do nome longo"; kill $PID; else echo "SERVIDOR: CAIU"; fi
python3 - "$W/opendhcp.state" <<'PY'
import socket, struct, sys
b = open(sys.argv[1], "rb").read()
print(f"registros de 104 bytes: {len(b) / 104}")
for k in range(0, len(b) - 103, 104):
    r = b[k:k + 104]
    print("  ", ":".join(f"{x:02x}" for x in r[24:30]), socket.inet_ntoa(r[8:12]), r[40:104].split(b"\0")[0].decode())
PY
