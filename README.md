# rush

**rush** is a rush implementation of [Ush Python (ush.py)](https://github.com/lspm-pkg/ush.py) — a dependency-free WebSocket terminal relay for **Linux and Windows**.

It stays intentionally unauthenticated. Run it only on a trusted network or behind an authenticated TLS reverse proxy.

## Features

- Dependency-free: pure Python 3 standard library, no `websockets` / `requests`
- Strict websocket frame validation and bounded queues
- PTY backpressure so slow clients can't exhaust memory
- Proper cleanup for sockets, PTYs, and child login processes
- Cross-platform terminal size detection
- Service installer for systemd / OpenRC (`-si`)

## Install

### Linux

```sh
sudo wget -O /usr/bin/rush https://github.com/YOUR_USER/rush/releases/latest/download/rush.py
sudo chmod +x /usr/bin/rush
```

Or run straight from the repo:

```sh
python3 rush.py <host> [-p 8080]
```

### Windows

```pwsh
curl.exe -L --output C:\Windows\System32\rush.exe https://github.com/YOUR_USER/rush/releases/latest/download/rush.py
```

(Run as Administrator.)

## Usage

```sh
# server (Linux)
rush -s -p 8080

# client
rush ws(s)://host [-p 8080] [-v]

# install + enable systemd/OpenRC service (Linux, as root)
rush -si -p 8080
```

Disconnect with `Ctrl+]`.

## Compatibility

- v4.0 is not compatible with v1.x, v2.x, or v3.x clients or servers.
- Update and restart the server first, then update every client.

## Credits

Based on [ush.py](https://github.com/lspm-pkg/ush.py) by lspm-pkg.
