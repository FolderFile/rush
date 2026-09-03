<img width="1332" height="692" alt="image-shadow" src="https://github.com/user-attachments/assets/be1c435d-18ea-45ac-a272-8ef5c8ac74c1" />

# rush

rush is a remote terminal over websockets. Server on a Linux box, client from
anywhere, shell in your terminal. Written in Rust with zero dependencies:
no crates, no interpreter, no runtime, one binary.

It is a Rust implementation of ush and speaks the same wire protocol. The
compatibility is one-directional by design: a rush client connects to ush.py
v4.0 servers fine, but ush.py clients are refused by rush servers (rush
checks its own user agent at the handshake, ush.py clients fail there).

## Install

Linux:
```bash
curl -fL https://github.com/FolderFile/rush/releases/latest/download/install.sh | bash
```
That puts the binary in /usr/bin/rush after checking it against SHA256SUMS.
The repo is private, so if curl gets a 404 the script falls back to gh auth
or GITHUB_TOKEN.

Or the same way ush installs itself, one line as root:
```bash
wget -O /usr/bin/rush https://github.com/FolderFile/rush/releases/latest/download/rush-linux; chmod +x /usr/bin/rush
```
While the repo is private that wget needs the browser download or gh, since
releases are not public. `rush --update` upgrades the binary later,
`rush --uninstall` removes it. On Windows, take rush.exe from the releases
page.

## Build
```bash
cargo build --release
```
That is the whole toolchain story. The result in target/release/rush is all
you need.

## Usage

Server (Linux):
```bash
rush -s -p 8080
```
Client (Linux or Windows):
```bash
rush thehost -p 8080      ip, ush-style, defaults to port 8080
rush thehost:8080         explicit port
rush thehost              domain, defaults to port 80
```
A bare domain goes out on port 80, which is what you want behind a
cloudflared tunnel; this box's own public server runs that way at
rush-sh.venesus.xyz. On a normal root-run server you get whatever login
prompt the machine shows (rush runs /bin/login, same as ush), username and
password included. `Ctrl+]` disconnects.

Run a single command instead of a shell, ssh style:
```bash
rush thehost -e "uname -a"
```
The client exits with the remote command's exit status. If the link is
unreliable, add `-r` and the client will retry a dropped connection five
times with backoff.

As root on the server, `rush -si -p 8080` copies the binary to /usr/bin/rush
and installs a systemd or OpenRC service, whichever it finds.

## Token

By default anyone who can reach the port gets a login prompt. To require a
shared secret:
```bash
rush -s -p 8080 -k mysecret
rush thehost -p 8080 -k mysecret
```
or set RUSH_KEY on both sides. Wrong tokens get dropped after a one second
delay, the comparison is constant time, and sessions run with a scrubbed
environment (TERM and PATH only) so the token cannot leak into a shell.

This is not real authentication. Behind a TLS proxy it is fine, on a hostile
network it is not.

## How it works

The server answers websocket upgrades on a plain TCP port, waits for a resize
message, then forks /bin/login on a pty ($SHELL as fallback, or whatever
RUSH_SHELL points at, useful in containers). From there it is a dumb pipe:
binary websocket frames carry terminal bytes, text frames carry small JSON
control messages like resizes. The exact wire format is in PROTOCOL.md.

The client puts your terminal in raw mode and forwards every byte, Ctrl+C
included. The only key it keeps for itself is Ctrl+].

## Known issues

- Server is Linux only. Client works on Linux and Windows.
- No TLS yet, so no wss://. Put it behind caddy or nginx if you need
  encryption.
- The token travels in cleartext before the upgrade. Same rule: TLS proxy.
- Window resizes are polled every 500ms instead of using SIGWINCH.
- `-e` sessions only work rush to rush. Against a ush.py server you just get
  the normal login shell.
