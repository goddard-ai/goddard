//! Host process inspection shared by the daemon's parent watchdog and the
//! migration sweep that abandons only dead migrators' artifacts.

#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Windows reuses process ids, so the handle is opened for the narrowest
/// right that answers the question and closed immediately. A pid that no
/// longer exists fails to open; one that has exited but is still held open by
/// another handle reports an exit code instead of `STILL_ACTIVE`.
#[cfg(windows)]
pub fn is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0_u32;
        let read = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        // A failed read leaves the process's state unknown; assuming it lives
        // is the safer error than acting on a live process as if it were gone.
        read == 0 || exit_code == STILL_ACTIVE as u32
    }
}

#[cfg(not(any(unix, windows)))]
pub fn is_alive(_pid: u32) -> bool {
    true
}
