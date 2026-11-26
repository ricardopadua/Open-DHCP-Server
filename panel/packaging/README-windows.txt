Open DHCP for Windows
=====================

OpenDHCP.exe opens the Open DHCP panel in its own window (no browser). The panel runs inside
the app, so no separate service is needed for it.

Requirements
- Windows 10 or 11 (64-bit) with the Microsoft Edge WebView2 Runtime (already included in
  Windows 11 and in up-to-date Windows 10; otherwise install it from microsoft.com/edge/webview2).
- Open DHCP Server for Windows installed as the "OpenDHCPServer" service.
- Run as administrator (the app asks for it): controlling the service and editing the .ini need it.

First run
- The app creates its configuration at %APPDATA%\OpenDHCP\panel.toml, assuming the server is in
  C:\OpenDHCPServer (OpenDHCPServer.ini, log\OpenDHCPServer%Y%m%d.log). If yours is elsewhere,
  the app shows the file to edit; fix the paths and open it again.
- Sign in with user "admin" and password "admin", then change it in "My account".
- Keep WebView2Loader.dll next to OpenDHCP.exe.

opendhcp-panel.exe is the same panel without a window, for running it as a background service
(opendhcp-panel.exe serve -c panel.toml) and opening it in a browser at the configured address.

Known limitations
- The lease list reads the Linux 64-bit state file layout; with the Windows server the list of
  active leases may not be available. Reservations, ranges, options, backups, the activity log
  and service control work from the .ini, the log and the service.
