use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::json;
use crate::pty;
use crate::sys;
use crate::ws::{self, BoundedQueue, Msg, WsStream};

const READ_CHUNK: usize = 16384;

pub fn run(bind: &str, port: u16, token: Option<String>) -> Result<(), String> {
    let listener = TcpListener::bind((bind, port)).map_err(|e| format!("cannot bind {bind}:{port}: {e}"))?;
    println!("[rush] v{} server running on :{}", crate::VERSION, port);
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let token = token.clone();
                std::thread::spawn(move || {
                    let _ = handle(s, token.as_deref());
                });
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, token: Option<&str>) -> Result<(), String> {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(ws::HANDSHAKE_TIMEOUT));
    ws::server_handshake(&mut stream, token).map_err(|e| e.to_string())?;
    let ws = Arc::new(WsStream::new(stream, false).map_err(|e| e.to_string())?);
    ws.set_read_timeout(Some(ws::HANDSHAKE_TIMEOUT));
    let init_text = match ws.read_message() {
        Ok(Msg::Text(text)) => text,
        Ok(_) => return Err("resize message required".into()),
        Err(e) => return Err(e.to_string()),
    };
    ws.set_read_timeout(None);
    let init = json::parse_session_init(&init_text);

    let session = PtySession {
        ws,
        stop: Arc::new(AtomicBool::new(false)),
        queue: Arc::new(BoundedQueue::new(ws::MAX_QUEUE)),
    };
    let pty = match &init.exec {
        Some(cmd) => pty::spawn_command(cmd, init.rows, init.cols),
        None => pty::spawn(init.rows, init.cols),
    }
    .map_err(|e| format!("pty: {e}"))?;
    session.run(pty);
    Ok(())
}

struct PtySession {
    ws: Arc<WsStream>,
    stop: Arc<AtomicBool>,
    queue: Arc<BoundedQueue>,
}

impl PtySession {
    fn run(&self, session: pty::Pty) {
        let master = session.master;
        let pid = session.pid;

        let reader_stop = Arc::clone(&self.stop);
        let reader_queue = Arc::clone(&self.queue);
        let pty_reader = std::thread::spawn(move || loop {
            let mut buf = vec![0u8; READ_CHUNK];
            let n = pty::read(master, &mut buf);
            if n > 0 {
                if !reader_queue.push(Some(buf[..n as usize].to_vec()), &reader_stop) {
                    break;
                }
                continue;
            }
            if n == 0 {
                reader_queue.push(None, &reader_stop);
                break;
            }
            let err = sys::last_os_error();
            if err == sys::EAGAIN || err == sys::EINTR {
                if reader_stop.load(Ordering::SeqCst) {
                    break;
                }
                ws::poll_readable(master, 100);
                continue;
            }
            if err == sys::EIO {
                std::thread::sleep(Duration::from_millis(50));
                if reader_stop.load(Ordering::SeqCst) {
                    break;
                }
                if !ws::poll_readable(master, 100) {
                    continue;
                }
                continue;
            }
            reader_queue.push(None, &reader_stop);
            break;
        });

        let writer_ws = Arc::clone(&self.ws);
        let writer_stop = Arc::clone(&self.stop);
        let pty_writer = std::thread::spawn(move || {
            let mut pending: Vec<u8> = Vec::new();
            loop {
                match writer_ws.read_message() {
                    Ok(Msg::Binary(data)) => {
                        if pending.len() + data.len() > ws::MAX_PENDING_INPUT {
                            break;
                        }
                        pending.extend_from_slice(&data);
                        if !flush_input(master, &mut pending, &writer_stop) {
                            break;
                        }
                    }
                    Ok(Msg::Text(text)) => {
                        if let Some((rows, cols)) = json::parse_resize_command(&text) {
                            pty::set_winsize(master, rows, cols);
                            pty::kill_group(pid, sys::SIGWINCH);
                        }
                    }
                    Err(_) => break,
                }
                if writer_stop.load(Ordering::SeqCst) {
                    break;
                }
            }
            writer_stop.store(true, Ordering::SeqCst);
            writer_ws.close();
        });

        let pinger_ws = Arc::clone(&self.ws);
        let pinger_stop = Arc::clone(&self.stop);
        std::thread::spawn(move || loop {
            for _ in 0..ws::PING_INTERVAL * 10 {
                if pinger_stop.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = pinger_ws.ping();
        });

        while let Some(item) = self.queue.pop(&self.stop) {
            match item {
                Some(data) => {
                    if self.ws.send_binary(&data).is_err() {
                        break;
                    }
                }
                None => break,
            }
        }
        self.stop.store(true, Ordering::SeqCst);

        let mut exit_code: Option<i32> = None;
        let reap_deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(status) = pty::reap(pid, false) {
                exit_code = Some(if status & 0x7F == 0 {
                    (status >> 8) & 0xFF
                } else {
                    128 + (status & 0x7F)
                });
                break;
            }
            if Instant::now() >= reap_deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Some(code) = exit_code {
            let _ = self.ws.send_text(&json::exit_message(code));
        }
        self.ws.close();

        pty::close(master);
        pty::kill_group(pid, sys::SIGTERM);
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if pty::reap(pid, false).is_some() {
                break;
            }
            if Instant::now() >= deadline {
                pty::kill_group(pid, sys::SIGKILL);
                pty::reap(pid, true);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = pty_reader.join();
        let _ = pty_writer.join();
    }
}

fn flush_input(master: i32, pending: &mut Vec<u8>, stop: &AtomicBool) -> bool {
    while !pending.is_empty() {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        let n = pty::write(master, pending);
        if n > 0 {
            pending.drain(..n as usize);
            continue;
        }
        if n < 0 {
            let err = sys::last_os_error();
            if err == sys::EAGAIN || err == sys::EINTR {
                if !ws::poll_writable(master, 100) && !stop.load(Ordering::SeqCst) {
                    continue;
                }
                continue;
            }
            pending.clear();
            return true;
        }
    }
    true
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        let candidate = std::path::Path::new(dir).join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn install_service(port: u16) -> Result<(), String> {
    if !pty::running_as_root() {
        return Err("-si must be run as root".into());
    }
    let target = std::path::Path::new("/usr/bin/rush");
    let source = std::env::current_exe().map_err(|e| e.to_string())?;
    let same = std::fs::canonicalize(target)
        .ok()
        .zip(std::fs::canonicalize(&source).ok())
        .map(|(a, b)| a == b)
        .unwrap_or(false);
    if !same {
        std::fs::copy(&source, target).map_err(|e| format!("cannot install to /usr/bin/rush: {e}"))?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755));
    }
    let target_str = target.display().to_string();

    if which("systemctl").is_some() && std::path::Path::new("/run/systemd/system").is_dir() {
        let unit = format!(
            "[Unit]\nDescription=rush remote shell server\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nEnvironment=TERM=xterm-256color\nExecStart={} --server -p {}\nRestart=on-failure\nRestartSec=2\nLimitNOFILE=65536\n\n[Install]\nWantedBy=multi-user.target\n",
            target_str, port
        );
        std::fs::write("/etc/systemd/system/rush.service", unit).map_err(|e| e.to_string())?;
        run_cmd(&["systemctl", "daemon-reload"])?;
        run_cmd(&["systemctl", "enable", "--now", "rush.service"])?;
    } else if which("rc-service").is_some() {
        let script = format!(
            "#!/sbin/openrc-run\nname=rush\ndescription=\"rush remote shell server\"\ncommand={}\ncommand_args=\"--server -p {}\"\ncommand_background=yes\npidfile=/run/rush.pid\noutput_log=/var/log/rush.log\nerror_log=/var/log/rush.log\nretry=\"TERM/10/KILL/5\"\n\nexport TERM=xterm-256color\n",
            target_str, port
        );
        std::fs::write("/etc/init.d/rush", script).map_err(|e| e.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions("/etc/init.d/rush", std::fs::Permissions::from_mode(0o755));
        run_cmd(&["rc-update", "add", "rush", "default"])?;
        run_cmd(&["rc-service", "rush", "start"])?;
    } else {
        return Err("Neither systemd nor OpenRC was detected".into());
    }
    println!("Installed and enabled rush service on port {}.", port);
    Ok(())
}

fn run_cmd(argv: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new(argv[0])
        .args(&argv[1..])
        .status()
        .map_err(|e| format!("service setup failed: {}: {}", argv.join(" "), e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("service setup failed: {}", argv.join(" ")))
    }
}
