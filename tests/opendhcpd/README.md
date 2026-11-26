# opendhcpd robustness tests

Sends valid and malformed DHCP packets to `opendhcpd` and checks that it keeps serving. They run
without root, inside a throwaway user + network namespace with a dummy interface
(192.168.50.1/24), so nothing touches the real network.

```sh
g++ -O2 -fPIE -pie -fstack-protector-strong -o /tmp/opendhcpd opendhcpd.cpp -lpthread
unshare -r -n sh tests/opendhcpd/run.sh /tmp/opendhcpd /tmp/odhcp-test        # malformed packets + status page
unshare -r -n sh tests/opendhcpd/run-cycle.sh /tmp/opendhcpd /tmp/odhcp-cycle # DISCOVER/REQUEST/ACK, restart, long hostname
```

Expected: `SERVIDOR: vivo` at the end of both, no `<script>` in the status page, hostnames in the
log without line breaks or HTML, and a 104-byte record per lease in the state file.

- `run.sh`: a normal request; a hostname with a line break and HTML; `hlen=255`; option 57 =
  65535 with large echoed options; an option longer than the packet; numeric options with a wrong
  size; a long HTTP path.
- `run-cycle.sh`: a full lease, a restart that reloads the state file, then the same MAC with a
  64-character hostname.
