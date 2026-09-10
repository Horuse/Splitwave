use std::fs::OpenOptions;
use std::io;
use std::path::Path;

#[cfg(unix)]
mod platform {
    use std::os::fd::IntoRawFd;
    use std::sync::atomic::{AtomicI32, Ordering};

    use super::*;

    static CRASH_FD: AtomicI32 = AtomicI32::new(-1);

    macro_rules! report {
        ($signal:literal) => {
            concat!(
                "{\"kind\":\"nativeCrash\",\"message\":\"Native crash: ",
                $signal,
                "\",\"backtrace\":\"The process terminated before a Rust backtrace could be captured. Use the OS crash dump for the native stack.\",\"thread\":\"<native>\",\"version\":\"",
                env!("CARGO_PKG_VERSION"),
                "\"}\n"
            )
            .as_bytes()
        };
    }

    const SIGABRT_REPORT: &[u8] = report!("SIGABRT");
    const SIGBUS_REPORT: &[u8] = report!("SIGBUS");
    const SIGFPE_REPORT: &[u8] = report!("SIGFPE");
    const SIGILL_REPORT: &[u8] = report!("SIGILL");
    const SIGSEGV_REPORT: &[u8] = report!("SIGSEGV");
    const SIGSYS_REPORT: &[u8] = report!("SIGSYS");
    const SIGTRAP_REPORT: &[u8] = report!("SIGTRAP");

    extern "C" fn fatal_signal(signal: libc::c_int) {
        let report = match signal {
            libc::SIGABRT => SIGABRT_REPORT,
            libc::SIGBUS => SIGBUS_REPORT,
            libc::SIGFPE => SIGFPE_REPORT,
            libc::SIGILL => SIGILL_REPORT,
            libc::SIGSEGV => SIGSEGV_REPORT,
            libc::SIGSYS => SIGSYS_REPORT,
            libc::SIGTRAP => SIGTRAP_REPORT,
            _ => b"",
        };
        let fd = CRASH_FD.load(Ordering::Relaxed);
        if fd >= 0 && !report.is_empty() {
            let mut written = 0;
            while written < report.len() {
                let result = unsafe {
                    libc::write(
                        fd,
                        report[written..].as_ptr().cast(),
                        report.len() - written,
                    )
                };
                if result <= 0 {
                    break;
                }
                written += result as usize;
            }
        }
        if unsafe { libc::raise(signal) } != 0 {
            unsafe { libc::_exit(128 + signal) };
        }
    }

    fn register(signal: libc::c_int) -> io::Result<()> {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = fatal_signal as *const () as usize;
        action.sa_flags = libc::SA_RESETHAND;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        if unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn install(path: &Path) -> io::Result<()> {
        if CRASH_FD.load(Ordering::Relaxed) < 0 {
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            CRASH_FD.store(file.into_raw_fd(), Ordering::Relaxed);
        }
        reinstall()
    }

    pub fn reinstall() -> io::Result<()> {
        for signal in [
            libc::SIGABRT,
            libc::SIGBUS,
            libc::SIGFPE,
            libc::SIGILL,
            libc::SIGSEGV,
            libc::SIGSYS,
            libc::SIGTRAP,
        ] {
            register(signal)?;
        }
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub fn trigger() {
        unsafe { libc::raise(libc::SIGABRT) };
        std::process::abort();
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::io::IntoRawHandle;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::WriteFile;
    use windows::Win32::System::Diagnostics::Debug::{
        SetUnhandledExceptionFilter, EXCEPTION_CONTINUE_SEARCH, EXCEPTION_POINTERS,
    };

    use super::*;

    static CRASH_HANDLE: AtomicUsize = AtomicUsize::new(0);
    const REPORT: &[u8] = concat!("{\"kind\":\"nativeCrash\",\"message\":\"Unhandled native Windows exception\",\"backtrace\":\"The process terminated before a Rust backtrace could be captured. Use the Windows crash dump for the native stack.\",\"thread\":\"<native>\",\"version\":\"", env!("CARGO_PKG_VERSION"), "\"}\n").as_bytes();

    unsafe extern "system" fn unhandled_exception(_: *const EXCEPTION_POINTERS) -> i32 {
        let raw = CRASH_HANDLE.load(Ordering::Relaxed);
        if raw != 0 {
            let handle = HANDLE(raw as *mut _);
            let mut written = 0;
            let _ = unsafe { WriteFile(handle, Some(REPORT), Some(&mut written), None) };
        }
        EXCEPTION_CONTINUE_SEARCH
    }

    pub fn install(path: &Path) -> io::Result<()> {
        if CRASH_HANDLE.load(Ordering::Relaxed) == 0 {
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            CRASH_HANDLE.store(file.into_raw_handle() as usize, Ordering::Relaxed);
        }
        reinstall()
    }

    pub fn reinstall() -> io::Result<()> {
        unsafe { SetUnhandledExceptionFilter(Some(unhandled_exception)) };
        Ok(())
    }

    #[cfg(debug_assertions)]
    pub fn trigger() {
        unsafe { windows::Win32::System::Diagnostics::Debug::RaiseException(0xE053_5743, 1, None) };
        std::process::abort();
    }
}

pub fn install(path: &Path) -> io::Result<()> {
    platform::install(path)
}

pub fn reinstall() -> io::Result<()> {
    platform::reinstall()
}

#[cfg(debug_assertions)]
pub fn trigger() {
    platform::trigger()
}
