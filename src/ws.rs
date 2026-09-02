use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(target_os = "linux")]
use crate::crypto::ct_eq;
use crate::crypto::{base64, random_bytes, sha1};
#[cfg(target_os = "linux")]
use crate::sys;

pub const WS_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
pub const MAX_FRAME: usize = 1024 * 1024;
#[cfg(target_os = "linux")]
pub const MAX_QUEUE: usize = 64;
#[cfg(target_os = "linux")]
pub const MAX_PENDING_INPUT: usize = 1024 * 1024;
pub const PING_INTERVAL: u64 = 20;
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

const OP_TEXT: u8 = 1;
const OP_BINARY: u8 = 2;
const OP_CLOSE: u8 = 8;
const OP_PING: u8 = 9;
const OP_PONG: u8 = 10;

pub enum Msg {
    Text(String),
    Binary(Vec<u8>),
}

pub struct Uri {
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub fn parse_uri(raw: &str) -> Result<Uri, String> {
    let rest = match raw.strip_prefix("wss://") {
        Some(_) => return Err("wss:// is not supported by this build; put the server behind a TLS proxy and use ws://".into()),
        None => match raw.strip_prefix("ws://") {
            Some(r) => r,
            None => return Err("URL must start with ws://".into()),
        },
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err("missing host".into());
    }
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        match inner.split_once(']') {
            Some((h, tail)) => {
                let port = match tail.strip_prefix(':') {
                    Some(p) => p.parse::<u16>().map_err(|_| "invalid port".to_string())?,
                    None => 80,
                };
                (h.to_string(), port)
            }
            None => return Err("unterminated IPv6 literal".into()),
        }
    } else if let Some((h, p)) = authority.rsplit_once(':') {
        if h.contains(':') {
            return Err("invalid host".into());
        }
        (h.to_string(), p.parse::<u16>().map_err(|_| "invalid port".to_string())?)
    } else {
        (authority.to_string(), 80)
    };
    Ok(Uri { host, port, path: path.to_string() })
}

fn read_http_head(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let n = stream.read(&mut byte)?;
        if n == 0 {
            return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "connection closed during handshake"));
        }
        buf.push(byte[0]);
        if buf.len() > 16384 {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "request too large"));
        }
        if buf.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn parse_headers(head: &str) -> (String, Vec<(String, String)>) {
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    (request, headers)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .rev()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn accept_key(client_key: &str) -> String {
    let digest = sha1(format!("{}{}", client_key, WS_GUID).as_bytes());
    base64(&digest)
}

pub fn client_handshake(stream: &mut TcpStream, uri: &Uri, token: Option<&str>) -> Result<(), String> {
    let mut key_bytes = [0u8; 16];
    random_bytes(&mut key_bytes);
    let key = base64(&key_bytes);
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\nUser-Agent: rush/{}\r\n",
        uri.path, uri.host, key, crate::VERSION
    );
    if let Some(t) = token {
        request.push_str(&format!("Authorization: Bearer {}\r\n", t));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("handshake failed: {}", e))?;
    let head = read_http_head(stream).map_err(|e| format!("handshake failed: {}", e))?;
    let (status, headers) = parse_headers(&head);
    if !status.starts_with("HTTP/1.1 101") || header(&headers, "sec-websocket-accept") != Some(accept_key(&key).as_str()) {
        return Err("WebSocket handshake failed".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn server_handshake(stream: &mut TcpStream, token: Option<&str>) -> Result<(), std::io::Error> {
    let head = read_http_head(stream)?;
    let (request, headers) = parse_headers(&head);
    let key = header(&headers, "sec-websocket-key").unwrap_or("").to_string();
    let upgrade_ok = header(&headers, "upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);
    if !request.starts_with("GET ") || !upgrade_ok || key.is_empty() {
        return Err(std::io::Error::new(ErrorKind::InvalidData, "invalid upgrade"));
    }
    if let Some(t) = token {
        let ok = header(&headers, "authorization")
            .map(|v| ct_eq(v.as_bytes(), format!("Bearer {}", t).as_bytes()))
            .unwrap_or(false);
        if !ok {
            std::thread::sleep(Duration::from_secs(1));
            return Err(std::io::Error::new(ErrorKind::PermissionDenied, "bad token"));
        }
    }
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(&key)
    );
    stream.write_all(response.as_bytes())?;
    Ok(())
}

pub struct WsStream {
    reader: Mutex<std::io::BufReader<TcpStream>>,
    writer: Mutex<TcpStream>,
    client: bool,
    closed: AtomicBool,
}

impl WsStream {
    pub fn new(stream: TcpStream, client: bool) -> std::io::Result<WsStream> {
        Ok(WsStream {
            reader: Mutex::new(std::io::BufReader::with_capacity(16384, stream.try_clone()?)),
            writer: Mutex::new(stream),
            client,
            closed: AtomicBool::new(false),
        })
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) {
        if let Ok(r) = self.reader.lock() {
            let _ = r.get_ref().set_read_timeout(timeout);
        }
    }

    fn read_frame(&self) -> std::io::Result<(bool, u8, Vec<u8>)> {
        let mut r = self.reader.lock().unwrap_or_else(|e| e.into_inner());
        let mut hdr = [0u8; 2];
        r.read_exact(&mut hdr)?;
        let fin = hdr[0] & 0x80 != 0;
        let opcode = hdr[0] & 0x0F;
        let masked = hdr[1] & 0x80 != 0;
        let mut len = (hdr[1] & 0x7F) as usize;
        if masked == self.client {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "invalid masking"));
        }
        if len == 126 {
            let mut ext = [0u8; 2];
            r.read_exact(&mut ext)?;
            len = u16::from_be_bytes(ext) as usize;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            r.read_exact(&mut ext)?;
            len = u64::from_be_bytes(ext) as usize;
        }
        if len > MAX_FRAME || (opcode >= 8 && (!fin || len > 125)) {
            return Err(std::io::Error::new(ErrorKind::InvalidData, "invalid frame size"));
        }
        let mut mask = [0u8; 4];
        if masked {
            r.read_exact(&mut mask)?;
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        Ok((fin, opcode, payload))
    }

    pub fn read_message(&self) -> std::io::Result<Msg> {
        let mut fragments: Vec<u8> = Vec::new();
        let mut msg_type: Option<u8> = None;
        loop {
            let (fin, opcode, payload) = self.read_frame()?;
            match opcode {
                OP_CLOSE => {
                    if !self.closed.swap(true, Ordering::SeqCst) {
                        let _ = self.write_frame(OP_CLOSE, &payload[..payload.len().min(125)]);
                    }
                    return Err(std::io::Error::new(ErrorKind::ConnectionAborted, "peer closed"));
                }
                OP_PING => {
                    self.write_frame(OP_PONG, &payload)?;
                    continue;
                }
                OP_PONG => continue,
                OP_TEXT | OP_BINARY => {
                    if msg_type.is_some() {
                        return Err(std::io::Error::new(ErrorKind::InvalidData, "new message during fragmentation"));
                    }
                    msg_type = Some(opcode);
                }
                0 => {
                    if msg_type.is_none() {
                        return Err(std::io::Error::new(ErrorKind::InvalidData, "unexpected continuation"));
                    }
                }
                _ => {
                    return Err(std::io::Error::new(ErrorKind::InvalidData, "unsupported opcode"));
                }
            }
            fragments.extend_from_slice(&payload);
            if fragments.len() > MAX_FRAME {
                return Err(std::io::Error::new(ErrorKind::InvalidData, "message too large"));
            }
            if fin {
                return match msg_type {
                    Some(OP_TEXT) => String::from_utf8(fragments)
                        .map(Msg::Text)
                        .map_err(|_| std::io::Error::new(ErrorKind::InvalidData, "invalid utf-8")),
                    Some(OP_BINARY) => Ok(Msg::Binary(fragments)),
                    _ => Err(std::io::Error::new(ErrorKind::InvalidData, "malformed message")),
                };
            }
        }
    }

    fn write_frame(&self, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
        if self.closed.load(Ordering::SeqCst) && opcode != OP_CLOSE {
            return Err(std::io::Error::new(ErrorKind::ConnectionAborted, "socket closed"));
        }
        let len = payload.len();
        let mut header = vec![0x80 | opcode];
        let mask_bit = if self.client { 0x80 } else { 0 };
        if len < 126 {
            header.push(len as u8 | mask_bit);
        } else if len <= 65535 {
            header.push(126 | mask_bit);
            header.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            header.push(127 | mask_bit);
            header.extend_from_slice(&(len as u64).to_be_bytes());
        }
        let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        if self.client {
            let mut mask = [0u8; 4];
            random_bytes(&mut mask);
            header.extend_from_slice(&mask);
            w.write_all(&header)?;
            let mut masked = payload.to_vec();
            for (i, b) in masked.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
            w.write_all(&masked)?;
        } else {
            w.write_all(&header)?;
            w.write_all(payload)?;
        }
        w.flush()
    }

    pub fn send_text(&self, text: &str) -> std::io::Result<()> {
        self.write_frame(OP_TEXT, text.as_bytes())
    }

    pub fn send_binary(&self, data: &[u8]) -> std::io::Result<()> {
        self.write_frame(OP_BINARY, data)
    }

    pub fn ping(&self) -> std::io::Result<()> {
        self.write_frame(OP_PING, &[])
    }

    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            let _ = self.write_frame(OP_CLOSE, &[]);
        }
        if let Ok(w) = self.writer.lock() {
            let _ = w.shutdown(Shutdown::Both);
        }
    }
}

#[cfg(target_os = "linux")]
pub struct BoundedQueue {
    items: Mutex<std::collections::VecDeque<Option<Vec<u8>>>>,
    cap: usize,
    signal: Mutex<bool>,
    cond: std::sync::Condvar,
}

#[cfg(target_os = "linux")]
impl BoundedQueue {
    pub fn new(cap: usize) -> BoundedQueue {
        BoundedQueue {
            items: Mutex::new(std::collections::VecDeque::new()),
            cap,
            signal: Mutex::new(false),
            cond: std::sync::Condvar::new(),
        }
    }

    pub fn push(&self, item: Option<Vec<u8>>, stop: &AtomicBool) -> bool {
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            if items.len() < self.cap {
                items.push_back(item);
                self.cond.notify_one();
                return true;
            }
            let (guard, _) = self
                .cond
                .wait_timeout(items, Duration::from_millis(100))
                .unwrap_or_else(|e| e.into_inner());
            items = guard;
        }
    }

    pub fn pop(&self, stop: &AtomicBool) -> Option<Option<Vec<u8>>> {
        let mut items = self.items.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(item) = items.pop_front() {
                self.cond.notify_one();
                return Some(item);
            }
            if *self.signal.lock().unwrap_or_else(|e| e.into_inner()) {
                return None;
            }
            if stop.load(Ordering::SeqCst) {
                return None;
            }
            let (guard, _) = self
                .cond
                .wait_timeout(items, Duration::from_millis(100))
                .unwrap_or_else(|e| e.into_inner());
            items = guard;
        }
    }

    pub fn finish(&self) {
        *self.signal.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.cond.notify_all();
    }
}

#[cfg(target_os = "linux")]
pub fn poll_readable(fd: i32, timeout_ms: i32) -> bool {
    unsafe { sys::poll_one(fd, sys::POLLIN, timeout_ms) > 0 }
}

#[cfg(target_os = "linux")]
pub fn poll_writable(fd: i32, timeout_ms: i32) -> bool {
    unsafe { sys::poll_one(fd, sys::POLLOUT, timeout_ms) > 0 }
}
