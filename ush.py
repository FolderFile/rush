#!/usr/bin/env python3
"""ush.py v4.0 - dependency-free WebSocket terminal relay.

This remains intentionally unauthenticated. Run it only on a trusted network
or behind an authenticated TLS reverse proxy.
"""
import argparse
import asyncio
import base64
import contextlib
import hashlib
import json
import os
import platform
import shutil
import signal
import ssl
import struct
import subprocess
import sys

VERSION = "4.0"
MAX_FRAME = 1024 * 1024
MAX_QUEUE = 64
MAX_PENDING_INPUT = 1024 * 1024
PING_INTERVAL = 20
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
IS_WIN = platform.system() == "Windows"

if not IS_WIN:
    import fcntl
    import select
    import termios
    import tty
else:
    import msvcrt


class ProtocolError(Exception):
    pass


class WebSocket:
    def __init__(self, reader, writer, client):
        self.reader = reader
        self.writer = writer
        self.client = client
        self.write_lock = asyncio.Lock()
        self.closed = False

    async def _read_frame(self):
        first, second = await self.reader.readexactly(2)
        fin, opcode = bool(first & 0x80), first & 0x0F
        masked, length = bool(second & 0x80), second & 0x7F
        # RFC 6455: clients mask frames and servers never do.
        if masked != (not self.client):
            raise ProtocolError("invalid masking")
        if length == 126:
            length = struct.unpack(">H", await self.reader.readexactly(2))[0]
        elif length == 127:
            length = struct.unpack(">Q", await self.reader.readexactly(8))[0]
        if length > MAX_FRAME or (opcode >= 8 and (not fin or length > 125)):
            raise ProtocolError("invalid frame size")
        mask = await self.reader.readexactly(4) if masked else None
        payload = await self.reader.readexactly(length)
        if mask:
            payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        return fin, opcode, payload

    async def recv(self):
        fragments = bytearray()
        message_type = None
        while True:
            fin, opcode, payload = await self._read_frame()
            if opcode == 8:
                if not self.closed:
                    await self._send_frame(8, payload[:125])
                self.closed = True
                raise ConnectionError("peer closed")
            if opcode == 9:
                await self._send_frame(10, payload)
                continue
            if opcode == 10:
                continue
            if opcode in (1, 2):
                if message_type is not None:
                    raise ProtocolError("new message during fragmentation")
                message_type = opcode
            elif opcode == 0:
                if message_type is None:
                    raise ProtocolError("unexpected continuation")
            else:
                raise ProtocolError("unsupported opcode")
            fragments.extend(payload)
            if len(fragments) > MAX_FRAME:
                raise ProtocolError("message too large")
            if fin:
                result = bytes(fragments)
                if message_type == 1:
                    return result.decode("utf-8")
                return result

    async def _send_frame(self, opcode, payload=b""):
        if self.closed and opcode != 8:
            raise ConnectionError("socket closed")
        length = len(payload)
        header = bytearray([0x80 | opcode])
        if length < 126:
            header.append(length | (0x80 if self.client else 0))
        elif length <= 65535:
            header.append(126 | (0x80 if self.client else 0))
            header.extend(struct.pack(">H", length))
        else:
            header.append(127 | (0x80 if self.client else 0))
            header.extend(struct.pack(">Q", length))
        if self.client:
            mask = os.urandom(4)
            header.extend(mask)
            payload = bytes(value ^ mask[index % 4] for index, value in enumerate(payload))
        async with self.write_lock:
            self.writer.write(header + payload)
            await self.writer.drain()

    async def send(self, message):
        if isinstance(message, str):
            await self._send_frame(1, message.encode())
        else:
            await self._send_frame(2, message)

    async def close(self):
        if not self.closed:
            with contextlib.suppress(Exception):
                await self._send_frame(8)
            self.closed = True
        self.writer.close()
        with contextlib.suppress(Exception):
            await self.writer.wait_closed()


def terminal_size():
    try:
        if IS_WIN:
            size = os.get_terminal_size()
            return size.lines, size.columns
        result = fcntl.ioctl(0, termios.TIOCGWINSZ, b"\0" * 8)
        if len(result) < 4:
            raise OSError("terminal size response is too short")
        return struct.unpack("HH", result[:4])
    except (OSError, struct.error):
        return 24, 80


def safe_size(rows, cols):
    return max(1, min(int(rows), 1000)), max(1, min(int(cols), 5000))


async def connect_websocket(uri):
    secure = uri.startswith("wss://")
    if not (secure or uri.startswith("ws://")):
        raise ValueError("URL must start with ws:// or wss://")
    target = uri[6 if secure else 5:]
    authority, separator, suffix = target.partition("/")
    path = "/" + suffix if separator else "/"
    if authority.startswith("["):
        host, port = authority[1:].split("]", 1)
        port = int(port[1:]) if port else (443 if secure else 80)
    elif ":" in authority:
        host, port_text = authority.rsplit(":", 1)
        port = int(port_text)
    else:
        host, port = authority, 443 if secure else 80
    key = base64.b64encode(os.urandom(16)).decode()
    context = ssl.create_default_context() if secure else None
    reader, writer = await asyncio.open_connection(host, port, ssl=context, server_hostname=host if secure else None)
    host_header = authority or host
    request = (
        f"GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUpgrade: websocket\r\n"
        "Connection: Upgrade\r\nSec-WebSocket-Version: 13\r\n"
        f"Sec-WebSocket-Key: {key}\r\nUser-Agent: ush.py/{VERSION}\r\n\r\n"
    )
    writer.write(request.encode("ascii"))
    await writer.drain()
    response = await reader.readuntil(b"\r\n\r\n")
    lines = response.decode("iso-8859-1").split("\r\n")
    headers = {line.split(":", 1)[0].lower(): line.split(":", 1)[1].strip() for line in lines[1:] if ":" in line}
    expected = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
    if not lines[0].startswith("HTTP/1.1 101") or headers.get("sec-websocket-accept") != expected:
        writer.close()
        raise ConnectionError("WebSocket handshake failed")
    return WebSocket(reader, writer, True)


async def client(host, port, verbose):
    uri = host if host.startswith(("ws://", "wss://")) else f"ws://{host}:{port}"
    stop = asyncio.Event()
    old_term = None
    try:
        ws = await connect_websocket(uri)
        rows, cols = terminal_size()
        await ws.send(json.dumps({"type": "resize", "rows": rows, "cols": cols}))

        async def receive():
            try:
                while not stop.is_set():
                    message = await ws.recv()
                    if isinstance(message, bytes):
                        sys.stdout.buffer.write(message)
                        sys.stdout.buffer.flush()
            finally:
                stop.set()

        async def send_input():
            loop = asyncio.get_running_loop()
            if IS_WIN:
                while not stop.is_set():
                    if msvcrt.kbhit():
                        char = msvcrt.getwch()
                        if char == "\x1d":
                            break
                        await ws.send(b"\x7f" if char == "\x08" else char.encode("utf-8", "ignore"))
                    else:
                        await asyncio.sleep(0.01)
            else:
                def read_input():
                    return os.read(0, 4096) if select.select([0], [], [], 0.2)[0] else b""
                while not stop.is_set():
                    data = await loop.run_in_executor(None, read_input)
                    if b"\x1d" in data:
                        break
                    if data:
                        await ws.send(data)
            stop.set()

        async def resize():
            previous = (rows, cols)
            while not stop.is_set():
                await asyncio.sleep(0.5)
                current = terminal_size()
                if current != previous:
                    previous = current
                    await ws.send(json.dumps({"type": "resize", "rows": current[0], "cols": current[1]}))

        async def keepalive():
            while not stop.is_set():
                await asyncio.sleep(PING_INTERVAL)
                with contextlib.suppress(Exception):
                    await ws._send_frame(9)

        if not IS_WIN:
            old_term = termios.tcgetattr(0)
            tty.setraw(0)
            current = termios.tcgetattr(0)
            current[3] &= ~termios.ISIG
            termios.tcsetattr(0, termios.TCSADRAIN, current)
        tasks = [asyncio.create_task(coro()) for coro in (receive, send_input, resize, keepalive)]
        await stop.wait()
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        await ws.close()
    except Exception as error:
        print(f"Fail: {error}" if verbose else "Fail", file=sys.stderr)
    finally:
        if old_term is not None:
            termios.tcsetattr(0, termios.TCSADRAIN, old_term)
        print("Connection closed.")


async def serve_client(ws):
    loop = asyncio.get_running_loop()
    master = slave = pid = None
    output = asyncio.Queue(MAX_QUEUE)
    input_buffer = bytearray()
    reader_active = False
    writer_active = False

    def remove_pty_reader():
        nonlocal reader_active
        if reader_active:
            loop.remove_reader(master)
            reader_active = False

    def remove_pty_writer():
        nonlocal writer_active
        if writer_active:
            loop.remove_writer(master)
            writer_active = False

    def pty_readable():
        nonlocal reader_active
        try:
            while not output.full():
                data = os.read(master, 16384)
                if not data:
                    output.put_nowait(None)
                    remove_pty_reader()
                    return
                output.put_nowait(data)
        except BlockingIOError:
            pass
        except OSError:
            with contextlib.suppress(asyncio.QueueFull):
                output.put_nowait(None)
            remove_pty_reader()
        if output.full():
            remove_pty_reader()  # TCP backpressure instead of unbounded RAM use.

    def pty_writable():
        nonlocal writer_active
        try:
            while input_buffer:
                written = os.write(master, input_buffer)
                del input_buffer[:written]
        except BlockingIOError:
            return
        except OSError:
            input_buffer.clear()
        loop.remove_writer(master)
        writer_active = False

    def enqueue_input(data):
        nonlocal writer_active
        if len(input_buffer) + len(data) > MAX_PENDING_INPUT:
            raise ProtocolError("PTY input limit reached")
        input_buffer.extend(data)
        if not writer_active:
            loop.add_writer(master, pty_writable)
            writer_active = True

    async def read_ws():
        while True:
            message = await ws.recv()
            if isinstance(message, bytes):
                enqueue_input(message)
                continue
            try:
                command = json.loads(message)
                if command.get("type") == "resize":
                    rows, cols = safe_size(command["rows"], command["cols"])
                    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
                    os.killpg(pid, signal.SIGWINCH)
            except (KeyError, TypeError, ValueError, OSError):
                continue

    async def write_ws():
        nonlocal reader_active
        while True:
            data = await output.get()
            if data is None:
                return
            await ws.send(data)
            if not reader_active and not output.full():
                loop.add_reader(master, pty_readable)
                reader_active = True

    async def reap():
        await loop.run_in_executor(None, os.waitpid, pid, 0)

    async def keepalive():
        while True:
            await asyncio.sleep(PING_INTERVAL)
            with contextlib.suppress(Exception):
                await ws._send_frame(9)

    try:
        init = await asyncio.wait_for(ws.recv(), timeout=10)
        if not isinstance(init, str):
            raise ProtocolError("resize message required")
        command = json.loads(init)
        rows, cols = safe_size(command.get("rows", 24), command.get("cols", 80))
        master, slave = os.openpty()
        fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        pid = os.fork()
        if pid == 0:
            os.close(master)
            # os.login_tty is unavailable in some distro Python builds.
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
            os.dup2(slave, 0)
            os.dup2(slave, 1)
            os.dup2(slave, 2)
            if slave > 2:
                os.close(slave)
            os.environ["TERM"] = "xterm-256color"
            os.execv("/bin/login", ["/bin/login"])
        os.close(slave)
        slave = None
        os.set_blocking(master, False)
        loop.add_reader(master, pty_readable)
        reader_active = True
        tasks = [asyncio.create_task(coro()) for coro in (read_ws, write_ws, reap, keepalive)]
        _, pending = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        for task in pending:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
    except (ConnectionError, ProtocolError, asyncio.TimeoutError, json.JSONDecodeError, OSError):
        pass
    finally:
        if master is not None:
            remove_pty_reader()
            remove_pty_writer()
            with contextlib.suppress(OSError):
                os.close(master)
        if slave is not None:
            with contextlib.suppress(OSError):
                os.close(slave)
        if pid:
            with contextlib.suppress(ProcessLookupError):
                os.killpg(pid, signal.SIGTERM)
            try:
                await asyncio.wait_for(loop.run_in_executor(None, os.waitpid, pid, 0), timeout=3)
            except (asyncio.TimeoutError, ChildProcessError, OSError):
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(pid, signal.SIGKILL)
                with contextlib.suppress(ChildProcessError, OSError):
                    await loop.run_in_executor(None, os.waitpid, pid, 0)
        await ws.close()


async def server(port):
    if platform.system() != "Linux":
        sys.exit("Server runs on Linux only.")
    listener = await asyncio.start_server(lambda r, w: accept(r, w), "0.0.0.0", port, backlog=512)
    print(f"[ush] v{VERSION} server running on :{port}", flush=True)
    async with listener:
        await listener.serve_forever()


async def accept(reader, writer):
    try:
        request = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), timeout=10)
        if len(request) > 16384:
            raise ProtocolError("request too large")
        lines = request.decode("iso-8859-1").split("\r\n")
        headers = {line.split(":", 1)[0].lower(): line.split(":", 1)[1].strip() for line in lines[1:] if ":" in line}
        key = headers.get("sec-websocket-key")
        if not lines[0].startswith("GET ") or headers.get("upgrade", "").lower() != "websocket" or not key:
            raise ProtocolError("invalid upgrade")
        accept_key = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
        writer.write(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                      f"Sec-WebSocket-Accept: {accept_key}\r\n\r\n").encode("ascii"))
        await writer.drain()
        await serve_client(WebSocket(reader, writer, False))
    except (asyncio.TimeoutError, asyncio.IncompleteReadError, asyncio.LimitOverrunError, ProtocolError, UnicodeDecodeError, OSError):
        writer.close()
        with contextlib.suppress(Exception):
            await writer.wait_closed()


def install_service(port):
    if os.geteuid() != 0:
        sys.exit("-si must be run as root")
    target = "/usr/bin/ush"
    source = os.path.realpath(__file__)
    if os.path.realpath(target) != source:
        shutil.copy2(source, target)
        os.chmod(target, 0o755)
    if shutil.which("systemctl") and os.path.isdir("/run/systemd/system"):
        path = "/etc/systemd/system/ush.service"
        content = f"""[Unit]
Description=ush.py remote shell server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
Environment=TERM=xterm-256color
ExecStart={target} --server -p {port}
Restart=on-failure
RestartSec=2
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
"""
        manager = ["systemctl", "daemon-reload"]
        enable = ["systemctl", "enable", "--now", "ush.service"]
    elif shutil.which("rc-service"):
        path = "/etc/init.d/ush"
        content = f"""#!/sbin/openrc-run
name=ush
description="ush.py remote shell server"
command={target}
command_args="--server -p {port}"
command_background=yes
pidfile=/run/ush.pid
output_log=/var/log/ush.log
error_log=/var/log/ush.log
retry="TERM/10/KILL/5"

export TERM=xterm-256color
"""
        manager = []
        enable = ["rc-update", "add", "ush", "default"]
    else:
        sys.exit("Neither systemd nor OpenRC was detected")
    with open(path, "w", encoding="ascii") as service_file:
        service_file.write(content)
    if path.endswith("/ush"):
        os.chmod(path, 0o755)
    for command in (manager, enable):
        if command:
            result = subprocess.run(command, check=False)
            if result.returncode:
                sys.exit(f"service setup failed: {' '.join(command)}")
    if path.endswith("/ush"):
        result = subprocess.run(["rc-service", "ush", "start"], check=False)
        if result.returncode:
            sys.exit("service installed but could not be started")
    print(f"Installed and enabled ush service on port {port}.")


def main():
    parser = argparse.ArgumentParser(description=f"ush.py v{VERSION}")
    parser.add_argument("--server", "-s", action="store_true", help="run the Linux server")
    parser.add_argument("-si", action="store_true", help="install and enable a server service")
    parser.add_argument("-p", type=int, default=8080, help="server port (default: 8080)")
    parser.add_argument("-v", "--verbose", action="store_true")
    parser.add_argument("host", nargs="?", help="server host or ws(s) URL")
    args = parser.parse_args()
    if not 1 <= args.p <= 65535:
        parser.error("port must be between 1 and 65535")
    if args.si:
        install_service(args.p)
    elif args.server:
        asyncio.run(server(args.p))
    elif args.host:
        asyncio.run(client(args.host, args.p, args.verbose))
    else:
        parser.print_help()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
