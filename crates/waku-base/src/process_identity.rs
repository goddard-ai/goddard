//! Instance checks for signals sent to owned child processes.

/// The kernel's birth stamp, compared only on the same host and platform.
pub fn start_time(pid: i32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        // SAFETY: the output buffer has the size and alignment required by proc_pidinfo.
        let read = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                std::mem::size_of::<libc::proc_bsdinfo>() as i32,
            )
        };
        if read != i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()? {
            return None;
        }
        // SAFETY: a complete proc_bsdinfo was returned above.
        let info = unsafe { info.assume_init() };
        Some(
            info.pbi_start_tvsec
                .saturating_mul(1_000_000)
                .saturating_add(info.pbi_start_tvusec),
        )
    }
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.get(stat.rfind(')')? + 2..)?
            .split_whitespace()
            .nth(19)?
            .parse()
            .ok()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = pid;
        None
    }
}

/// Match a non-secret launch marker in argv, without returning or logging argv/environment.
pub fn has_argument(pid: i32, marker: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut bytes = vec![0_u8; 1024 * 1024];
        let mut len = bytes.len();
        // SAFETY: mib and the writable output buffer remain valid for the synchronous sysctl.
        let result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                mib.len() as u32,
                bytes.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 || len < 4 {
            return false;
        }
        bytes.truncate(len);
        let count = i32::from_ne_bytes(bytes[..4].try_into().unwrap_or_default());
        let Some(end) = bytes[4..].iter().position(|byte| *byte == 0) else {
            return false;
        };
        let mut offset = 4 + end;
        while bytes.get(offset) == Some(&0) {
            offset += 1;
        }
        bytes[offset..]
            .split(|byte| *byte == 0)
            .take(usize::try_from(count).unwrap_or(0))
            .any(|arg| arg == marker.as_bytes())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|bytes| {
            bytes
                .split(|byte| *byte == 0)
                .any(|arg| arg == marker.as_bytes())
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (pid, marker);
        false
    }
}
