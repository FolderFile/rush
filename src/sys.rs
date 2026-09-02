#![allow(non_camel_case_types)]

#[cfg(target_os = "linux")]
pub type pid_t = i32;

#[cfg(target_os = "linux")]
pub const EAGAIN: i32 = 11;
#[cfg(target_os = "linux")]
pub const EINTR: i32 = 4;

#[cfg(target_os = "linux")]
pub const O_RDWR: i32 = 2;
#[cfg(target_os = "linux")]
pub const O_NOCTTY: i32 = 0o400;
#[cfg(target_os = "linux")]
pub const O_NONBLOCK: i32 = 0o4000;
#[cfg(target_os = "linux")]
pub const F_GETFL: i32 = 3;
#[cfg(target_os = "linux")]
pub const F_SETFL: i32 = 4;

#[cfg(target_os = "linux")]
pub const TIOCSCTTY: u64 = 0x540E;
#[cfg(target_os = "linux")]
pub const TIOCGWINSZ: u64 = 0x5413;
#[cfg(target_os = "linux")]
pub const TIOCSWINSZ: u64 = 0x5414;

#[cfg(target_os = "linux")]
pub const TCSADRAIN: i32 = 1;

#[cfg(target_os = "linux")]
pub const SIGKILL: i32 = 9;
#[cfg(target_os = "linux")]
pub const SIGTERM: i32 = 15;
#[cfg(target_os = "linux")]
pub const SIGWINCH: i32 = 28;

#[cfg(target_os = "linux")]
pub const WNOHANG: i32 = 1;

#[cfg(target_os = "linux")]
pub const POLLIN: i16 = 0x001;
#[cfg(target_os = "linux")]
pub const POLLOUT: i16 = 0x004;

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 32],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PollFd {
    pub fd: i32,
    pub events: i16,
    pub revents: i16,
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    extern "C" {
        pub fn posix_openpt(flags: i32) -> i32;
        pub fn grantpt(fd: i32) -> i32;
        pub fn unlockpt(fd: i32) -> i32;
        pub fn ptsname_r(fd: i32, buf: *mut u8, len: usize) -> i32;
        pub fn open(path: *const core::ffi::c_char, flags: i32, ...) -> i32;
        pub fn close(fd: i32) -> i32;
        pub fn read(fd: i32, buf: *mut u8, count: usize) -> isize;
        pub fn write(fd: i32, buf: *const u8, count: usize) -> isize;
        pub fn dup2(old: i32, new: i32) -> i32;
        pub fn fork() -> pid_t;
        pub fn setsid() -> i32;
        pub fn execv(path: *const core::ffi::c_char, argv: *const *const core::ffi::c_char) -> i32;
        pub fn _exit(code: i32) -> !;
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
        pub fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        pub fn kill(pid: pid_t, sig: i32) -> i32;
        pub fn waitpid(pid: pid_t, status: *mut i32, options: i32) -> pid_t;
        pub fn setenv(name: *const core::ffi::c_char, value: *const core::ffi::c_char, overwrite: i32) -> i32;
        pub fn access(path: *const core::ffi::c_char, mode: i32) -> i32;
        pub fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
        pub fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
        pub fn tcsetattr(fd: i32, actions: i32, termios: *const Termios) -> i32;
        pub fn cfmakeraw(termios: *mut Termios);
        pub fn geteuid() -> u32;
    }

    pub unsafe fn ioctl_ptr(fd: i32, request: u64, arg: *mut core::ffi::c_void) -> i32 {
        ioctl(fd, request, arg)
    }

    pub unsafe fn ioctl_winsize(fd: i32, request: u64, ws: &Winsize) -> i32 {
        ioctl(fd, request, ws as *const Winsize)
    }

    pub unsafe fn fcntl_getfl(fd: i32) -> i32 {
        fcntl(fd, F_GETFL)
    }

    pub unsafe fn fcntl_setfl(fd: i32, flags: i32) -> i32 {
        fcntl(fd, F_SETFL, flags)
    }

    pub unsafe fn poll_one(fd: i32, events: i16, timeout_ms: i32) -> i32 {
        let mut pfd = PollFd { fd, events, revents: 0 };
        poll(&mut pfd, 1, timeout_ms)
    }

    pub unsafe fn wait_pid(pid: pid_t, block: bool) -> Option<i32> {
        let mut status: i32 = 0;
        let opts = if block { 0 } else { WNOHANG };
        let r = waitpid(pid, &mut status, opts);
        if r == pid {
            Some(status)
        } else {
            None
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(windows)]
mod windows {
    pub type Handle = *mut core::ffi::c_void;

    pub const STD_INPUT_HANDLE: u32 = 0xFFFF_FFF6;
    pub const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;

    pub const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    pub const ENABLE_LINE_INPUT: u32 = 0x0002;
    pub const ENABLE_ECHO_INPUT: u32 = 0x0004;
    pub const ENABLE_WINDOW_INPUT: u32 = 0x0008;
    pub const ENABLE_MOUSE_INPUT: u32 = 0x0010;
    pub const ENABLE_QUICK_EDIT_MODE: u32 = 0x0040;
    pub const ENABLE_EXTENDED_FLAGS: u32 = 0x0080;
    pub const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;
    pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Coord {
        pub x: i16,
        pub y: i16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct SmallRect {
        pub left: i16,
        pub top: i16,
        pub right: i16,
        pub bottom: i16,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct ConsoleScreenBufferInfo {
        pub dw_size: Coord,
        pub dw_cursor_position: Coord,
        pub w_attributes: u16,
        pub sr_window: SmallRect,
        pub dw_maximum_window_size: Coord,
    }

    extern "system" {
        pub fn GetStdHandle(n_std_handle: u32) -> Handle;
        pub fn GetConsoleMode(h_console: Handle, lp_mode: *mut u32) -> i32;
        pub fn SetConsoleMode(h_console: Handle, dw_mode: u32) -> i32;
        pub fn GetConsoleScreenBufferInfo(
            h_console: Handle,
            lp_console_screen_buffer_info: *mut ConsoleScreenBufferInfo,
        ) -> i32;
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "linux")]
pub fn last_os_error() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}
