# Changelog

## 1.0.0

First stable release.

- websocket terminal relay: Linux server (pty + /bin/login), Linux and Windows clients
- wire-compatible with ush.py v4.0 sessions; rush servers additionally require the rush/ user agent and answer 403 to anything else
- exec mode (-e CMD, client exits with the remote status), reconnect (-r), shared token (-k / RUSH_KEY), bind address (-b)
- targets: ip (port 8080), host:port, bare domain (port 80, cloudflared-friendly)
- --update with SHA256SUMS verification, --uninstall, -si systemd/OpenRC installer, install.sh curl|bash installer
- security: constant-time token compare, 1s damping on bad tokens, scrubbed child environment (TERM + PATH only), no allocations between fork and exec
- statically linked musl Linux binary, runs on any x86_64 distro

## 0.6.0-alpha

- static musl Linux builds (fixes GLIBC errors on old distros)

## 0.5.0-alpha

- bare domains default to port 80

## 0.4.0-alpha

- rush servers refuse non-rush clients

## 0.3.0-alpha

- security audit fixes: scrubbed child environment, fork/exec safety, update checksum verification

## 0.2.0-alpha

- exec mode, reconnect, lifecycle commands, token auth

## 0.1.0-alpha

- first Rust port of the ush.py v4.0 system
