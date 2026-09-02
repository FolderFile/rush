pub fn size() -> (u16, u16) {
    terminal_size_impl()
}

#[cfg(target_os = "linux")]
fn terminal_size_impl() -> (u16, u16) {
    use crate::sys;
    let mut ws = sys::Winsize::default();
    let r = unsafe { sys::ioctl_ptr(0, sys::TIOCGWINSZ, &mut ws as *mut _ as *mut core::ffi::c_void) };
    if r == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (24, 80)
    }
}

#[cfg(windows)]
fn terminal_size_impl() -> (u16, u16) {
    use crate::sys;
    unsafe {
        let handle = sys::GetStdHandle(sys::STD_OUTPUT_HANDLE);
        let mut info = sys::ConsoleScreenBufferInfo::default();
        if sys::GetConsoleScreenBufferInfo(handle, &mut info) != 0 {
            let rows = info.sr_window.bottom - info.sr_window.top + 1;
            let cols = info.sr_window.right - info.sr_window.left + 1;
            if rows > 0 && cols > 0 {
                (rows as u16, cols as u16)
            } else {
                (24, 80)
            }
        } else {
            (24, 80)
        }
    }
}

pub struct RawGuard {
    inner: RawGuardImpl,
}

impl RawGuard {
    pub fn new() -> std::io::Result<RawGuard> {
        Ok(RawGuard { inner: RawGuardImpl::new()? })
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        self.inner.restore();
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::super::sys;

    pub struct RawGuardImpl {
        saved: sys::Termios,
    }

    impl RawGuardImpl {
        pub fn new() -> std::io::Result<RawGuardImpl> {
            let mut saved = unsafe { std::mem::zeroed::<sys::Termios>() };
            if unsafe { sys::tcgetattr(0, &mut saved) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            let mut raw = saved;
            unsafe { sys::cfmakeraw(&mut raw) };
            if unsafe { sys::tcsetattr(0, sys::TCSADRAIN, &raw) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(RawGuardImpl { saved })
        }

        pub fn restore(&mut self) {
            unsafe { sys::tcsetattr(0, sys::TCSADRAIN, &self.saved) };
        }
    }
}

#[cfg(windows)]
mod platform {
    use crate::sys;

    pub struct RawGuardImpl {
        saved_in: u32,
        saved_out: u32,
        active: bool,
    }

    impl RawGuardImpl {
        pub fn new() -> std::io::Result<RawGuardImpl> {
            unsafe {
                let hin = sys::GetStdHandle(sys::STD_INPUT_HANDLE);
                let hout = sys::GetStdHandle(sys::STD_OUTPUT_HANDLE);
                let mut in_mode = 0u32;
                let mut out_mode = 0u32;
                if sys::GetConsoleMode(hin, &mut in_mode) == 0 || sys::GetConsoleMode(hout, &mut out_mode) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let raw_in = (in_mode
                    & !(sys::ENABLE_PROCESSED_INPUT
                        | sys::ENABLE_LINE_INPUT
                        | sys::ENABLE_ECHO_INPUT
                        | sys::ENABLE_WINDOW_INPUT
                        | sys::ENABLE_MOUSE_INPUT
                        | sys::ENABLE_QUICK_EDIT_MODE))
                    | sys::ENABLE_EXTENDED_FLAGS
                    | sys::ENABLE_VIRTUAL_TERMINAL_INPUT;
                let raw_out = out_mode | sys::ENABLE_VIRTUAL_TERMINAL_PROCESSING;
                if sys::SetConsoleMode(hin, raw_in) == 0 || sys::SetConsoleMode(hout, raw_out) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(RawGuardImpl { saved_in: in_mode, saved_out: out_mode, active: true })
            }
        }

        pub fn restore(&mut self) {
            if self.active {
                unsafe {
                    sys::SetConsoleMode(sys::GetStdHandle(sys::STD_INPUT_HANDLE), self.saved_in);
                    sys::SetConsoleMode(sys::GetStdHandle(sys::STD_OUTPUT_HANDLE), self.saved_out);
                }
                self.active = false;
            }
        }
    }
}

#[cfg(target_os = "linux")]
use platform::RawGuardImpl;
#[cfg(windows)]
use platform::RawGuardImpl;
