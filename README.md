# Open DHCP Server

[Open DHCP Server](http://dhcpserver.sourceforge.net/) is a free, open-source DHCP server by
**Achal Dhir** (GNU GPL v2). This repository is a fork of
[ss-plus/Open-DHCP-Server](https://github.com/ss-plus/Open-DHCP-Server) that keeps the original
server (`opendhcpd`) unchanged and adds a modern way to run and manage it:

- **Real-time web panel:** active leases, range usage, service status and the server log, updated
  live, plus usage over time (24 hours / 7 days).
- **Configuration from the panel:**
  - reservations, with a free IP suggestion, conflict checks and CSV import/export;
  - ranges, with a CIDR prefix, ranges by first IP + count and subnet checks;
  - global options;
  - backups of `opendhcp.ini`, with compare and restore.
- **Open DHCP desktop app:** the panel in its own window on Linux and Windows, no browser needed.
- **Devices:** vendor by MAC, nicknames, notes and a per-device history.
- **Notifications** that can be dismissed, an on-disk history, Portuguese/English/French/Spanish,
  light and dark themes, and a mobile-friendly layout.

## Download

Get the latest version from [Releases](https://github.com/ricardopadua/Open-DHCP-Server/releases/latest):

| Package | For |
|---|---|
| `.deb` | Debian / Ubuntu |
| `.rpm` | Fedora / RHEL / openSUSE |
| `.tar.gz` | other Linux distributions (`sudo ./install.sh`) |
| `.AppImage` | any Linux distribution: the desktop app, which offers to install the DHCP server on first run |
| `.zip` | Windows 10/11 |

The Linux packages install:

- `opendhcpd` (64-bit) in `/opt/opendhcp`;
- the panel as a service (`opendhcp-panel`, started on install);
- the **Open DHCP** app in the applications menu.

Sign in with `admin` / `admin` and change the password in *My account*. The DHCP service itself is
not started automatically: review `opendhcp.ini` in the panel and press *Start*.

## Repository layout

- `opendhcpd.cpp`, `opendhcpd.h`, `opendhcp.ini`: the original Open DHCP Server (unchanged).
- [`panel/`](panel/README.md): the web panel (Rust); documentation in Portuguese.
- `panel/desktop/`: the Open DHCP desktop app.
- `panel/packaging/`: release packaging (`build-release.sh`).

## Security fixes in this fork

The original `opendhcpd` (about 10 years old) had memory-safety bugs reachable by any device on
the LAN, without authentication, in a process that runs as root. This fork fixes them in
`opendhcpd.cpp` without changing how the server is configured or used:

- **Packet parsing:**
  - a hardware address longer than 16 bytes is rejected;
  - options are checked against the packet size and their expected length;
  - the reply size requested by the client is capped.
- **Hostnames:**
  - a single fixed-size slot per lease, so loading the state file or replication can no longer
    overflow or free it;
  - restricted to letters, digits, `-` and `_`, which blocks log forging and HTML in the status
    page.
- **Logs:** bounded writes into the log buffer (every `sprintf` became `snprintf`).
- **HTTP status page:**
  - listens on `127.0.0.1` by default;
  - the allowed-client list is checked before reading the request;
  - the request buffer is terminated correctly;
  - the page buffer has room for its fixed footer.
- **Leases:** `DHCPDECLINE` locks an address for one hour instead of forever, and leases received
  through replication are capped.
- **Files:**
  - the state file is in `/var/lib/opendhcp` instead of `/tmp`;
  - files are opened without following symlinks;
  - `umask 022` instead of `0`.
- **Build:** PIE, stack protector, full RELRO and non-executable stack. The systemd units drop
  every root capability except binding port 67.

The web panel runs as its own unprivileged user (`opendhcp-panel`) and only accepts connections
from the same computer (`127.0.0.1`). A polkit rule lets it start, stop and restart the DHCP
service and nothing else. Release builds fail if `cargo audit` finds a known vulnerability in
the panel's dependencies.

Replication between two servers is still not authenticated: only enable `[REPLICATION_SERVERS]`
on a trusted network. Regression tests for malformed packets are in
[`tests/opendhcpd/`](tests/opendhcpd/README.md).

## Credits and license

- **Open DHCP Server** © 2005 Achal Dhir, licensed under the GNU GPL v2
  ([dhcpserver.sourceforge.net](http://dhcpserver.sourceforge.net/)). The `OpenDHCPServerManual.pdf`
  and the original `README` are part of that project.
- Fork of [ss-plus/Open-DHCP-Server](https://github.com/ss-plus/Open-DHCP-Server).
- The web panel, the desktop app and the packaging in this repository are also released under the
  GNU GPL v2 (see `LICENSE`).

## About the original Open DHCP Server

### Overview
[Open DHCP Server](http://dhcpserver.sourceforge.net/) is an Open Source Freeware. It has most of the Industry Standard Features. It  attempts to comply with various RFC for Dynamic Host Configuration Protocol. It is released under the  GNU Open Source License Agreement. 

Although it has most of the features and capabilities required for use by experts, it is still very simple to configure and use. It is also well suited for small and home use by persons not having any knowledge of DHCP Protocol. The most salient features include:

- Supports upto 125 DHCP ranges. 
- Supports both Static hosts and Dynamic DHCP Ranges. 
- Supports BOOTP and relay agents. 
- Supports multiple zones in single server. 
- Supports all possible DHCP Options straightaway, no need to define option types. 
- Supports filtering of DHCP Ranges on Mac Range, Vendor Options and User Options. 
- Options can be Global, DHCP Range Specific or Client specific. 
- Supports Replicated Fail Safe DHCP Operation. 
- Multiple ways to assign option values like strings, IPs, hex strings or byte arrays. 
- Lease Status is displayed on auto refreshing html page. 
- Low CPU and executable size, requiring very little memory.
