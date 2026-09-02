# rush

rush is a remote terminal over websockets. You run the server on a Linux box,
you connect to it from your terminal, and you get a shell. It's written in Rust
with zero dependencies - no crates, no runtime, one binary.

It's a rewrite of [ush.py](https://github.com/lspm-pkg/ush.py), a Python tool
that does the same thing. rush speaks the same protocol, so a rush client can
talk to a ush.py v4.0 server and the other way around. The difference is what's
under the hood: no interpreter, no event loop, no per-byte Python loops, and a
binary you can drop anywhere.

The name is what it is: ush, but in Rust.

## Building

You need a Rust toolchain, that's it. There is nothing else to install, ever.

    cargo build --release

The binary ends up in `target/release/rush` and is all you need.

## Usage

Start the server on the machine you want to reach (Linux):

    rush -s -p 8080

Connect from anywhere (Linux or Windows):

    rush thehost -p 8080

You'll get whatever login prompt the machine normally shows. Disconnect with
`Ctrl+]`.

If you want it to survive reboots, as root on the server:

    rush -si -p 8080

That copies the binary to /usr/bin/rush and installs a systemd or OpenRC
service, whichever it finds.

### Shared token

By default the server trusts whoever can reach the port, same as ush.py. If
that's too loose for your network, both sides can take a shared token:

    rush -s -p 8080 -k mysecret
    rush thehost -p 8080 -k mysecret

Clients without the token get dropped at the handshake. This is a padlock on a
garden gate, not real authentication - still put it behind a TLS proxy if the
network isn't yours.

## How it works

The server listens on a TCP port and speaks plain HTTP until someone asks for
a websocket upgrade. Once that's done it waits for a resize message, opens a
pty, and forks /bin/login onto it (or $SHELL if there's no login, or whatever
RUSH_SHELL points at if you set it - handy in containers). From then on the
session is a dumb pipe: binary websocket frames carry terminal bytes in both
directions, text frames carry small JSON control messages like window resizes.

On the client side rush puts your terminal in raw mode and everything you type
goes to the server as it's pressed, keys and all. `Ctrl+]` is the only key the
client keeps for itself. The wire format is documented in
[PROTOCOL.md](PROTOCOL.md) if you want to talk to it from something else.

## Known limitations

- The server only runs on Linux. The client runs on Linux and Windows.
- There is no TLS in this build, so no wss:// yet. Put it behind a TLS proxy
  (caddy, nginx, anything) if you need encryption.
- The token check happens before the websocket upgrade, in cleartext. Behind a
  TLS proxy that's fine; on a hostile network it isn't.
- Resize is polled every half second instead of using SIGWINCH. It works, but
  it's not subtle.
- `-e` exec sessions only work rush-to-rush; against a ush.py server the
  client just gets a normal login shell.

## Credits

rush is a rewrite of ush.py by lspm-pkg, which is where the protocol, the
design and most of the good ideas come from. rush is MIT licensed; ush.py
belongs to its author.
