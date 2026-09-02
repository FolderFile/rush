use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::json;
#[cfg(target_os = "linux")]
use crate::sys;
use crate::term;
use crate::ws::{self, Msg};

pub fn run(host: &str, port: u16, verbose: bool, token: Option<String>) {
    match client(host, port, token.as_deref()) {
        Ok(()) => {}
        Err(e) => {
            if verbose {
                eprintln!("Fail: {}", e);
            } else {
                eprintln!("Fail");
            }
        }
    }
    println!("Connection closed.");
}

fn client(host: &str, port: u16, token: Option<&str>) -> Result<(), String> {
    let uri_string = if host.starts_with("ws://") || host.starts_with("wss://") {
        host.to_string()
    } else {
        format!("ws://{}:{}", host, port)
    };
    let uri = ws::parse_uri(&uri_string)?;
    let mut stream = TcpStream::connect((uri.host.as_str(), uri.port))
        .map_err(|e| format!("cannot connect to {}: {}", uri.host, e))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(ws::HANDSHAKE_TIMEOUT));
    ws::client_handshake(&mut stream, &uri, token).map_err(|e| e)?;

    let _raw = term::RawGuard::new().map_err(|e| format!("terminal: {e}"))?;
    let ws = Arc::new(ws::WsStream::new(stream, true).map_err(|e| e.to_string())?);
    ws.set_read_timeout(None);
    let (rows, cols) = term::size();
    ws.send_text(&json::resize_message(rows, cols))
        .map_err(|e| format!("send: {e}"))?;

    let stop = Arc::new(AtomicBool::new(false));

    let out_ws = Arc::clone(&ws);
    let out_stop = Arc::clone(&stop);
    let output_thread = std::thread::spawn(move || {
        let stdout = std::io::stdout();
        loop {
            match out_ws.read_message() {
                Ok(Msg::Binary(data)) => {
                    let mut lock = stdout.lock();
                    if lock.write_all(&data).is_err() || lock.flush().is_err() {
                        break;
                    }
                }
                Ok(Msg::Text(text)) => {
                    let _ = text;
                    continue;
                }
                Err(_) => break,
            }
            if out_stop.load(Ordering::SeqCst) {
                break;
            }
        }
        out_stop.store(true, Ordering::SeqCst);
    });

    let in_ws = Arc::clone(&ws);
    let in_stop = Arc::clone(&stop);
    let input_thread = std::thread::spawn(move || {
        input_loop(&in_ws, &in_stop);
        in_stop.store(true, Ordering::SeqCst);
    });

    let resize_ws = Arc::clone(&ws);
    let resize_stop = Arc::clone(&stop);
    let resize_thread = std::thread::spawn(move || {
        let mut previous = (rows, cols);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            if resize_stop.load(Ordering::SeqCst) {
                break;
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

    let ping_ws = Arc::clone(&ws);
    let ping_stop = Arc::clone(&stop);
    let ping_thread = std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(ws::PING_INTERVAL));
        if ping_stop.load(Ordering::SeqCst) {
            break;
        }
        let _ = ping_ws.ping();
    });

    let _ = input_thread.join();
    stop.store(true, Ordering::SeqCst);
    ws.close();
    let _ = output_thread.join();
    let _ = resize_thread.join();
    let _ = ping_thread.join();
    Ok(())
}

#[cfg(target_os = "linux")]
fn input_loop(ws: &ws::WsStream, stop: &AtomicBool) {
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
            break;
        }
        if ws.send_binary(data).is_err() {
            break;
        }
    }
}

#[cfg(windows)]
fn input_loop(ws: &ws::WsStream, stop: &AtomicBool) {
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
