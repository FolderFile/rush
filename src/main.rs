mod client;
mod crypto;
mod json;
#[cfg(target_os = "linux")]
mod manage;
#[cfg(target_os = "linux")]
mod pty;
#[cfg(target_os = "linux")]
mod server;
mod sys;
mod term;
mod ws;

pub const VERSION: &str = "0.2.0-alpha";

const HELP: &str = "\
rush - remote terminal over websockets

usage:
  rush -s [-b ADDR] [-p PORT] [-k KEY]     run the server (Linux only)
  rush -si [-p PORT] [-k KEY]              install and enable a server service
  rush HOST [-p PORT] [-k KEY] [-v]        connect to a server
  rush HOST -e CMD                         run CMD on the server and exit
  rush --update                            update the installed binary (Linux)
  rush --uninstall                         remove the binary and service (Linux)

options:
  -s, --server       run the server
  -si                install and enable a systemd or OpenRC service
  -b, --bind ADDR    bind address (default: 0.0.0.0)
  -p PORT            server port (default: 8080)
  -k, --key KEY      shared token; or set RUSH_KEY
  -e, --exec CMD     run CMD instead of a login shell, exit when done
  -r, --reconnect    retry a dropped connection (5 attempts with backoff)
  -v, --verbose      show failure details
  -h, --help         show this help

HOST is an ip, a domain, or host:port. disconnect with Ctrl+]";

struct Args {
    server: bool,
    install: bool,
    update: bool,
    uninstall: bool,
    bind: String,
    port: u16,
    verbose: bool,
    reconnect: bool,
    token: Option<String>,
    exec: Option<String>,
    host: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        server: false,
        install: false,
        update: false,
        uninstall: false,
        bind: "0.0.0.0".to_string(),
        port: 8080,
        verbose: false,
        reconnect: false,
        token: None,
        exec: None,
        host: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-s" | "--server" => args.server = true,
            "-si" => args.install = true,
            "--update" => args.update = true,
            "--uninstall" => args.uninstall = true,
            "-v" | "--verbose" => args.verbose = true,
            "-r" | "--reconnect" => args.reconnect = true,
            "-h" | "--help" => {
                println!("{}", HELP);
                std::process::exit(0);
            }
            "--version" => {
                println!("rush {}", VERSION);
                std::process::exit(0);
            }
            "-p" => {
                let value = iter.next().ok_or("-p requires a port")?;
                args.port = value.parse().map_err(|_| format!("invalid port: {}", value))?;
            }
            "-b" | "--bind" => {
                args.bind = iter.next().ok_or("-b requires an address")?;
            }
            "-k" | "--key" => {
                args.token = Some(iter.next().ok_or("-k requires a token")?);
            }
            "-e" | "--exec" => {
                args.exec = Some(iter.next().ok_or("-e requires a command")?);
            }
            other => {
                if other.starts_with('-') && other.len() > 1 {
                    return Err(format!("unknown option: {}", other));
                }
                if args.host.is_some() {
                    return Err(format!("unexpected argument: {}", other));
                }
                args.host = Some(other.to_string());
            }
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("rush: {}\nrun 'rush --help' for usage", e);
            std::process::exit(2);
        }
    };
    if !(1..=65535).contains(&args.port) {
        eprintln!("rush: port must be between 1 and 65535");
        std::process::exit(2);
    }
    let token = args
        .token
        .clone()
        .filter(|t| !t.is_empty())
        .or_else(|| std::env::var("RUSH_KEY").ok().filter(|t| !t.is_empty()));

    if args.update || args.uninstall {
        #[cfg(target_os = "linux")]
        {
            let result = if args.update { manage::update() } else { manage::uninstall() };
            if let Err(e) = result {
                eprintln!("rush: {}", e);
                std::process::exit(1);
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = token;
            eprintln!("rush: --update and --uninstall are not supported on Windows; re-download rush.exe from the releases page.");
            std::process::exit(1);
        }
    } else if args.install || args.server {
        run_server(&args, token);
    } else {
        match args.host {
            Some(host) => client::run(&host, args.port, args.verbose, token, args.exec, args.reconnect),
            None => println!("{}", HELP),
        }
    }
}

#[cfg(target_os = "linux")]
fn run_server(args: &Args, token: Option<String>) {
    if args.install {
        if let Err(e) = server::install_service(args.port) {
            eprintln!("rush: {}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }
    if let Err(e) = server::run(&args.bind, args.port, token) {
        eprintln!("rush: {}", e);
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn run_server(_args: &Args, _token: Option<String>) {
    eprintln!("rush: the server runs on Linux only.");
    std::process::exit(1);
}
