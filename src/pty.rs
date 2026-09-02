#![cfg(target_os = "linux")]

use std::ffi::CString;
use std::io;

use crate::sys;

pub struct Pty {
    pub master: i32,
    pub pid: i32,
}

unsafe fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| CString::new("/bin/sh").unwrap())
}

unsafe fn exec_or_die(path: &str, arg0: &str) -> ! {
    let p = cstr(path);
    let a0 = cstr(arg0);
    let argv: [*const core::ffi::c_char; 2] = [a0.as_ptr(), std::ptr::null()];
    sys::execv(p.as_ptr(), argv.as_ptr());
    sys::_exit(127)
}

pub fn spawn(rows: u16, cols: u16) -> io::Result<Pty> {
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
    let name = std::str::from_utf8(&name_buf[..name_len]).unwrap_or("");
    let ws = sys::Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    unsafe { sys::ioctl_winsize(master, sys::TIOCSWINSZ, &ws) };

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
            let slave_path = cstr(name);
            let slave = sys::open(slave_path.as_ptr(), sys::O_RDWR, 0);
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
            let term = cstr("TERM");
            let val = cstr("xterm-256color");
            sys::setenv(term.as_ptr(), val.as_ptr(), 1);
            if let Ok(custom) = std::env::var("RUSH_SHELL") {
                if !custom.is_empty() {
                    let arg0 = custom.split('/').next_back().unwrap_or(&custom).to_string();
                    exec_or_die(&custom, &arg0);
                }
            }
            let login = cstr("/bin/login");
            if sys::access(login.as_ptr(), 1) == 0 {
                exec_or_die("/bin/login", "/bin/login");
            }
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
            let arg0 = shell.rsplit('/').next().unwrap_or(&shell).to_string();
            exec_or_die(&shell, &arg0);
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
