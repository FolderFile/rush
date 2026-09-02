# rush wire protocol

rush speaks the same protocol as ush.py v4.0. This document describes it
exactly, so you can implement a client or server without reading the code.

## Opening

The server listens on a TCP port. A session starts as an HTTP/1.1 upgrade:

    GET / HTTP/1.1
    Host: <host>
    Upgrade: websocket
    Connection: Upgrade
    Sec-WebSocket-Version: 13
    Sec-WebSocket-Key: <16 random bytes, base64>

    HTTP/1.1 101 Switching Protocols
    Upgrade: websocket
    Connection: Upgrade
    Sec-WebSocket-Accept: <base64(sha1(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"))>

The request path is not checked. The server must answer within 10 seconds or
the client gives up. If the server was started with `-k KEY`, the client must
add `Authorization: Bearer KEY` to the request or it gets dropped here.

Normal RFC 6455 framing rules apply: client frames are masked, server frames
are not, control frames carry at most 125 bytes, and no single frame may
exceed 1 MiB.

## First message

Within 10 seconds of the upgrade the client must send one text message:

    {"type": "resize", "rows": 24, "cols": 80}

The server reads rows and cols out of it (defaults 24x80 if missing) and clamps
them to 1-1000 rows and 1-5000 cols. If nothing arrives in time the server
drops the connection.

## Session

From here on the server has forked a login process on a pty, and the socket is
a dumb pipe:

- **Binary frames** carry terminal bytes, in both directions. The server
  buffers up to 1 MiB of undelivered input; pushing past that kills the
  session.
- **Text frames** carry control messages. The only one defined is resize:

      {"type": "resize", "rows": 33, "cols": 111}

  The server applies it to the pty and sends SIGWINCH to the login process
  group. Text messages that aren't a valid resize are ignored.

- **Ping/pong**: both sides send a ping every 20 seconds and answer pings with
  pongs. There is no keepalive timeout; a dead connection is noticed when a
  write or read fails.

Either side closing the websocket ends the session. The server then sends
SIGTERM to the process group, waits 3 seconds, and sends SIGKILL if it has to.

## Client behavior

The reference client keeps three things to itself and forwards everything
else: it translates the terminal into raw mode, polls the window size every
0.5 s and sends a resize when it changes, and treats byte 0x1D (`Ctrl+]`) as
"hang up now".
