#![cfg(target_os = "linux")]

use std::ffi::CString;
use std::io;

use crate::sys;

pub struct Pty {
    pub master: i32,
    pub pid: i32,
}

pub enum ChildKind {
    Login,
    Command(String),
}

struct ChildPlan {
    slave_name: CString,
    exec_path: CString,
    argv: Vec<CString>,
    envp: Vec<CString>,
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| CString::new("/bin/sh").unwrap())
}

fn argv_ptrs(v: &[CString]) -> Vec<*const core::ffi::c_char> {
    let mut out: Vec<*const core::ffi::c_char> = v.iter().map(|s| s.as_ptr()).collect();
    out.push(std::ptr::null());
    out
}

fn resolve_login_plan() -> CString {
    if let Ok(custom) = std::env::var("RUSH_SHELL") {
        if !custom.is_empty() {
            return cstr(&custom);
        }
    }
    let login = CString::new("/bin/login").unwrap();
    unsafe {
        if sys::access(login.as_ptr(), 1) == 0 {
            return login;
        }
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    cstr(&shell)
}

fn build_plan(kind: ChildKind) -> ChildPlan {
    match kind {
        ChildKind::Command(cmd) => {
            let cmd_c = cstr(&cmd);
            ChildPlan {
                slave_name: CString::new("").unwrap(),
                exec_path: CString::new("/bin/sh").unwrap(),
                argv: vec![cstr("sh"), cstr("-c"), cmd_c],
                envp: vec![
                    CString::new("TERM=xterm-256color").unwrap(),
                    CString::new("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin").unwrap(),
                ],
            }
        }
        ChildKind::Login => {
            let path = resolve_login_plan();
            let arg0 = path
                .to_string_lossy()
                .rsplit('/')
                .next()
                .unwrap_or("sh")
                .to_string();
            ChildPlan {
                slave_name: CString::new("").unwrap(),
                exec_path: path,
                argv: vec![cstr(&arg0)],
                envp: vec![
                    CString::new("TERM=xterm-256color").unwrap(),
                    CString::new("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin").unwrap(),
                ],
            }
        }
    }
}

fn open_pty(rows: u16, cols: u16) -> io::Result<(i32, String)> {
    let master = unsafe { sys::posix_openpt(sys::O_RDWR | sys::O_NOCTTY) };
    if master < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { sys::grantpt(master) } != 0 || unsafe { sys::unlockpt(master) } != 0 {
        let err = io::Error::last_os_error();
        unsafe { sys::close(master) };
        return Err(err);
    }
    let mut name_buf = [0u8; 128];
    if unsafe { sys::ptsname_r(master, name_buf.as_mut_ptr(), name_buf.len()) } != 0 {
        let err = io::Error::last_os_error();
        unsafe { sys::close(master) };
        return Err(err);
    }
    let name_len = name_buf.iter().position(|&b| b == 0).unwrap_or(0);
    let name = String::from_utf8_lossy(&name_buf[..name_len]).into_owned();
    let ws = sys::Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe { sys::ioctl_winsize(master, sys::TIOCSWINSZ, &ws) };
    Ok((master, name))
}

pub fn spawn_child(kind: ChildKind, rows: u16, cols: u16) -> io::Result<Pty> {
    let (master, slave_name) = open_pty(rows, cols)?;
    let mut plan = build_plan(kind);
    plan.slave_name = cstr(&slave_name);
    let argv = argv_ptrs(&plan.argv);
    let envp = argv_ptrs(&plan.envp);

    let pid = unsafe { sys::fork() };
    if pid < 0 {
        let err = io::Error::last_os_error();
        unsafe { sys::close(master) };
        return Err(err);
    }
    if pid == 0 {
        unsafe {
            sys::close(master);
            sys::setsid();
            let slave = sys::open(plan.slave_name.as_ptr(), sys::O_RDWR, 0);
            if slave < 0 {
                sys::_exit(127);
            }
            sys::ioctl(slave, sys::TIOCSCTTY, 0usize);
            sys::dup2(slave, 0);
            sys::dup2(slave, 1);
            sys::dup2(slave, 2);
            if slave > 2 {
                sys::close(slave);
            }
            sys::execve(plan.exec_path.as_ptr(), argv.as_ptr(), envp.as_ptr());
            sys::_exit(127)
        }
    }
    if unsafe { sys::fcntl_getfl(master) } < 0 {
        let err = io::Error::last_os_error();
        unsafe { sys::close(master) };
        return Err(err);
    }
    unsafe { sys::fcntl_setfl(master, sys::fcntl_getfl(master) | sys::O_NONBLOCK) };
    Ok(Pty { master, pid })
}

pub fn spawn(rows: u16, cols: u16) -> io::Result<Pty> {
    spawn_child(ChildKind::Login, rows, cols)
}

pub fn spawn_command(cmd: &str, rows: u16, cols: u16) -> io::Result<Pty> {
    spawn_child(ChildKind::Command(cmd.to_string()), rows, cols)
}

pub fn read(fd: i32, buf: &mut [u8]) -> isize {
    unsafe { sys::read(fd, buf.as_mut_ptr(), buf.len()) }
}

pub fn write(fd: i32, data: &[u8]) -> isize {
    unsafe { sys::write(fd, data.as_ptr(), data.len()) }
}

pub fn close(fd: i32) {
    unsafe { sys::close(fd) };
}

pub fn set_winsize(fd: i32, rows: u16, cols: u16) -> bool {
    let ws = sys::Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe { sys::ioctl_winsize(fd, sys::TIOCSWINSZ, &ws) == 0 }
}

pub fn kill_group(pid: i32, sig: i32) {
    unsafe { sys::kill(-pid, sig) };
}

pub fn reap(pid: i32, block: bool) -> Option<i32> {
    unsafe { sys::wait_pid(pid, block) }
}

pub fn running_as_root() -> bool {
    unsafe { sys::geteuid() == 0 }
}
