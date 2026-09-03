use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

use crate::json;
#[cfg(target_os = "linux")]
use crate::sys;
use crate::term;
use crate::ws;

const RECONNECT_ATTEMPTS: u32 = 5;

enum Outcome {
    UserQuit,
    RemoteExit(i32),
    TransportError,
}

pub fn run(host: &str, port: u16, verbose: bool, token: Option<String>, exec: Option<String>, reconnect: bool) {
    let mut backoff = 1u64;
    let mut outcome = Outcome::TransportError;
    for attempt in 0..RECONNECT_ATTEMPTS {
        outcome = session(host, port, token.as_deref(), exec.as_deref());
        match outcome {
            Outcome::UserQuit | Outcome::RemoteExit(_) => break,
            Outcome::TransportError => {
                if !reconnect || attempt + 1 == RECONNECT_ATTEMPTS {
                    break;
                }
                std::thread::sleep(Duration::from_secs(backoff));
                backoff *= 2;
            }
        }
    }
    match outcome {
        Outcome::RemoteExit(code) => {
            println!("Connection closed.");
            std::process::exit(code);
        }
        Outcome::UserQuit => println!("Connection closed."),
        Outcome::TransportError => {
            if verbose {
                eprintln!("Fail: connection lost");
            } else {
                eprintln!("Fail");
            }
            println!("Connection closed.");
        }
    }
}

fn split_host_port(host: &str, default_port: u16) -> (String, u16) {
    if host.starts_with("ws://") || host.starts_with("wss://") {
        return (host.to_string(), default_port);
    }
    if let Some(inner) = host.strip_prefix('[') {
        if let Some((h, tail)) = inner.split_once(']') {
            let port = tail
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(default_port);
            return (format!("[{}]", h), port);
        }
        return (host.to_string(), default_port);
    }
    if let Some((h, p)) = host.rsplit_once(':') {
        if !h.contains(':') && !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(port) = p.parse::<u16>() {
                return (h.to_string(), port);
            }
        }
    }
    let is_domain = host.chars().any(|c| c.is_ascii_alphabetic());
    let port = if is_domain { 80 } else { default_port };
    (host.to_string(), port)
}

fn session(host: &str, port: u16, token: Option<&str>, exec: Option<&str>) -> Outcome {
    let (host_arg, port) = split_host_port(host, port);
    let uri_string = if host_arg.starts_with("ws://") || host_arg.starts_with("wss://") {
        host_arg
    } else {
        format!("ws://{}:{}", host_arg, port)
    };
    let uri = match ws::parse_uri(&uri_string) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("rush: {}", e);
            return Outcome::UserQuit;
        }
    };
    let mut stream = match TcpStream::connect((uri.host.as_str(), uri.port)) {
        Ok(s) => s,
        Err(_) => return Outcome::TransportError,
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(ws::HANDSHAKE_TIMEOUT));
    if ws::client_handshake(&mut stream, &uri, token).is_err() {
        return Outcome::TransportError;
    }

    let raw = match term::RawGuard::new() {
        Ok(g) => g,
        Err(_) => return Outcome::TransportError,
    };
    let ws = match ws::WsStream::new(stream, true) {
        Ok(w) => std::sync::Arc::new(w),
        Err(_) => return Outcome::TransportError,
    };
    ws.set_read_timeout(None);
    let (rows, cols) = term::size();
    let greeting = match exec {
        Some(cmd) => json::exec_message(cmd, rows, cols),
        None => json::resize_message(rows, cols),
    };
    if ws.send_text(&greeting).is_err() {
        return Outcome::TransportError;
    }

    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let user_quit = std::sync::Arc::new(AtomicBool::new(false));
    let remote_code = std::sync::Arc::new(AtomicI32::new(-1));

    let out_ws = std::sync::Arc::clone(&ws);
    let out_stop = stop.clone();
    let out_code = remote_code.clone();
    let output_thread = std::thread::spawn(move || {
        let stdout = std::io::stdout();
        loop {
            match out_ws.read_message() {
                Ok(ws::Msg::Binary(data)) => {
                    let mut lock = stdout.lock();
                    if lock.write_all(&data).is_err() || lock.flush().is_err() {
                        break;
                    }
                }
                Ok(ws::Msg::Text(text)) => {
                    if let Some(code) = json::parse_exit_code(&text) {
                        out_code.store(code, Ordering::SeqCst);
                    }
                }
                Err(_) => break,
            }
            if out_stop.load(Ordering::SeqCst) {
                break;
            }
        }
        out_stop.store(true, Ordering::SeqCst);
    });

    let in_ws = std::sync::Arc::clone(&ws);
    let in_stop = stop.clone();
    let in_quit = user_quit.clone();
    let input_thread = std::thread::spawn(move || {
        input_loop(&in_ws, &in_stop, &in_quit);
        in_stop.store(true, Ordering::SeqCst);
    });

    let resize_ws = std::sync::Arc::clone(&ws);
    let resize_stop = stop.clone();
    let resize_thread = std::thread::spawn(move || {
        let mut previous = (rows, cols);
        loop {
            for _ in 0..5 {
                if resize_stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let current = term::size();
            if current != previous {
                previous = current;
                if resize_ws.send_text(&json::resize_message(current.0, current.1)).is_err() {
                    break;
                }
            }
        }
    });

    let ping_ws = std::sync::Arc::clone(&ws);
    let ping_stop = stop.clone();
    let ping_thread = std::thread::spawn(move || loop {
        for _ in 0..ws::PING_INTERVAL * 10 {
            if ping_stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = ping_ws.ping();
    });

    let _ = input_thread.join();
    stop.store(true, Ordering::SeqCst);
    ws.close();
    let _ = output_thread.join();
    let _ = resize_thread.join();
    let _ = ping_thread.join();
    drop(raw);

    if user_quit.load(Ordering::SeqCst) {
        Outcome::UserQuit
    } else {
        let code = remote_code.load(Ordering::SeqCst);
        if code >= 0 {
            Outcome::RemoteExit(code)
        } else {
            Outcome::TransportError
        }
    }
}

#[cfg(target_os = "linux")]
fn input_loop(ws: &ws::WsStream, stop: &AtomicBool, user_quit: &AtomicBool) {
    let mut buf = [0u8; 4096];
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if !ws::poll_readable(0, 200) {
            continue;
        }
        let n = unsafe { sys::read(0, buf.as_mut_ptr(), buf.len()) };
        if n <= 0 {
            break;
        }
        let data = &buf[..n as usize];
        if data.contains(&0x1D) {
            user_quit.store(true, Ordering::SeqCst);
            break;
        }
        if ws.send_binary(data).is_err() {
            break;
        }
    }
}

#[cfg(windows)]
fn input_loop(ws: &ws::WsStream, stop: &AtomicBool, user_quit: &AtomicBool) {
    use std::io::Read;
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    let mut buf = [0u8; 4096];
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match handle.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let mut data = buf[..n].to_vec();
                if data.contains(&0x1D) {
                    user_quit.store(true, Ordering::SeqCst);
                    break;
                }
                for b in data.iter_mut() {
                    if *b == 0x08 {
                        *b = 0x7F;
                    }
                }
                if ws.send_binary(&data).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
