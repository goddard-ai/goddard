//! Platform memory-pressure signals for runtime reclamation and containment.
//!
//! The guard samples normal/elevated/critical pressure on macOS and Linux.
//! Event watchers let other platforms shed idle runtimes early. Detection is
//! separate from runtime eligibility; unavailable signals remain explicit.

use crossbeam_channel::Receiver;

/// The pressure event stream — `None` when this platform's signal is
/// unavailable or could not be set up. Events mean "memory is tight right
/// now"; they are stateless, so a burst is the same shed pass as one.
pub fn watch() -> Option<Receiver<()>> {
    platform::watch()
}

/// Host pressure sampled off the UI thread. `None` means unavailable, not healthy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Normal,
    Elevated,
    Critical,
}

/// Read one host-pressure observation without spawning a watcher.
pub fn sample() -> Option<Level> {
    #[cfg(target_os = "macos")]
    {
        platform::level().map(macos_level)
    }
    #[cfg(target_os = "linux")]
    {
        let contents = std::fs::read_to_string("/proc/pressure/memory").ok()?;
        psi_some_avg10(&contents).map(linux_level)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Emergency admission/containment signal; sampled off the UI thread.
pub fn critical() -> Option<bool> {
    sample().map(|level| level == Level::Critical)
}

#[cfg(any(target_os = "macos", test))]
// Shed below 25% usable memory; keep the existing 15% emergency threshold.
fn macos_level(usable_percent: i32) -> Level {
    if usable_percent < 15 {
        Level::Critical
    } else if usable_percent < 25 {
        Level::Elevated
    } else {
        Level::Normal
    }
}

#[cfg(any(target_os = "linux", test))]
// PSI some avg10 measures the percentage of time tasks stalled on memory.
fn linux_level(some_avg10: f64) -> Level {
    if some_avg10 >= 50.0 {
        Level::Critical
    } else if some_avg10 > 20.0 {
        Level::Elevated
    } else {
        Level::Normal
    }
}

/// Spawn the shared watcher thread: poll `signal`, emit while it reports
/// pressure, keep the channel shallow so a burst stays one event.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn poll(signal: impl Fn() -> Option<bool> + Send + 'static) -> Option<Receiver<()>> {
    /// How often the poller re-reads its signal. Five seconds is far
    /// cheaper than a process table walk and still answers before pressure
    /// kills anything.
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
    // Setup fails when the first read does — the platform simply has no
    // signal; the reaper then keeps its ordinary age-based cadence.
    signal()?;
    let (events, receiver) = crossbeam_channel::bounded(1);
    std::thread::Builder::new()
        .name("waku-memory-pressure".into())
        .spawn(move || {
            loop {
                std::thread::sleep(POLL_INTERVAL);
                match signal() {
                    // Latest-only: a queued event is one shed pass, not one
                    // per poll tick the daemon slept through.
                    Some(true) => {
                        let _ = events.try_send(());
                    }
                    // A transient read failure keeps polling — the signal
                    // going away only matters when setup already proved it.
                    Some(false) | None => {}
                }
            }
        })
        .ok()?;
    Some(receiver)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::{Level, poll, sample};
    use crossbeam_channel::Receiver;

    /// `kern.memorystatus_level`: percentage of usable memory.
    pub(super) fn level() -> Option<i32> {
        let mut level: i32 = 0;
        let mut len = std::mem::size_of::<i32>();
        let ok = unsafe {
            libc::sysctlbyname(
                c"kern.memorystatus_level".as_ptr(),
                &mut level as *mut i32 as *mut core::ffi::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        (ok == 0).then_some(level)
    }

    pub fn watch() -> Option<Receiver<()>> {
        poll(|| sample().map(|level| level != Level::Normal))
    }
}

/// The `some avg10` field out of a `/proc/pressure/memory` body. Outside
/// the platform gate so the parse stays covered by a test on any OS.
#[cfg(any(target_os = "linux", test))]
fn psi_some_avg10(contents: &str) -> Option<f64> {
    contents
        .lines()
        .find(|line| line.starts_with("some "))
        .and_then(|line| {
            line.split_ascii_whitespace()
                .find_map(|field| field.strip_prefix("avg10="))
        })
        .and_then(|value| value.parse::<f64>().ok())
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{Level, poll, sample};
    use crossbeam_channel::Receiver;

    pub fn watch() -> Option<Receiver<()>> {
        poll(|| sample().map(|level| level != Level::Normal))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use crossbeam_channel::Receiver;
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Memory::{
        CreateMemoryResourceNotification, LowMemoryResourceNotification,
    };
    use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};

    /// `CreateMemoryResourceNotification` yields a waitable object the OS
    /// signals when physical memory runs low — an event, not a poll. The
    /// handle is process-scoped so the watcher owns it for the daemon's
    /// whole life.
    pub fn watch() -> Option<Receiver<()>> {
        let notification =
            unsafe { CreateMemoryResourceNotification(LowMemoryResourceNotification) };
        if notification.is_null() {
            return None;
        }
        // `HANDLE` is a raw pointer; carry it as a usize so the watcher
        // thread can own the wait.
        let notification = notification as usize;
        let (events, receiver) = crossbeam_channel::bounded(1);
        std::thread::Builder::new()
            .name("waku-memory-pressure".into())
            .spawn(move || {
                let notification = notification as *mut core::ffi::c_void;
                loop {
                    match unsafe { WaitForSingleObject(notification, INFINITE) } {
                        WAIT_OBJECT_0 => {
                            let _ = events.try_send(());
                        }
                        // A failed handle or abandoned wait ends the watcher —
                        // the reaper keeps its age-based cadence from then on.
                        _ => break,
                    }
                }
                unsafe {
                    CloseHandle(notification);
                }
            })
            .ok()?;
        Some(receiver)
    }
}

/// No supported signal — the caller falls back to age-based eviction.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod platform {
    use crossbeam_channel::Receiver;
    pub fn watch() -> Option<Receiver<()>> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pressure_severity_preserves_warning_and_emergency_boundaries() {
        use super::{Level::*, linux_level, macos_level};
        assert_eq!(macos_level(25), Normal);
        assert_eq!(macos_level(24), Elevated);
        assert_eq!(macos_level(15), Elevated);
        assert_eq!(macos_level(14), Critical);
        assert_eq!(linux_level(20.0), Normal);
        assert_eq!(linux_level(20.1), Elevated);
        assert_eq!(linux_level(49.9), Elevated);
        assert_eq!(linux_level(50.0), Critical);
    }

    #[test]
    fn psi_some_avg10_reads_the_some_line() {
        let body = "some avg10=24.56 avg60=9.10 avg300=3.21 total=123\n\
                    full avg10=88.0 avg60=1.0 avg300=0.0 total=0\n";
        assert_eq!(super::psi_some_avg10(body), Some(24.56));
        // The `full` line must not leak into the read, and a malformed body
        // is `None`, not a stall report.
        assert_eq!(super::psi_some_avg10("full avg10=99.0\n"), None);
        assert_eq!(super::psi_some_avg10("garbage\n"), None);
    }

    /// The host this codebase builds for exposes the level — a watcher
    /// that never reports would silently keep age-based eviction.
    #[cfg(target_os = "macos")]
    #[test]
    fn memorystatus_level_reports_on_this_host() {
        assert!(super::platform::level().is_some());
    }

    /// The watcher exists on supported hosts; the channel staying open
    /// matters more than any particular reading.
    #[test]
    fn watch_produces_a_live_channel() {
        assert!(super::watch().is_some());
    }
}
