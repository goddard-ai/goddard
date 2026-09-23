//! Host sleep inhibition, held while `DaemonSettings::keep_awake` is on.
//!
//! The daemon is only reachable while its host is awake, so a client that
//! wants to connect from the mobile or web apps needs the machine to refuse
//! idle sleep. On macOS this is the pair of assertions `caffeinate -is`
//! declares; on Windows the equivalent `SetThreadExecutionState` flags;
//! elsewhere it is a no-op until a backend exists.

/// A held sleep assertion. `Drop` releases it; construction is infallible —
/// a failed assertion logs and the guard still releases whatever took.
pub struct SleepAssertion {
    _imp: imp::Assertion,
}

impl SleepAssertion {
    /// Prevent this host from sleeping so the daemon stays reachable.
    /// `reason` shows in `pmset -g assertions` on macOS.
    pub fn acquire(reason: &str) -> Self {
        Self {
            _imp: imp::Assertion::acquire(reason),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;

    use objc2_foundation::NSString;

    type CFStringRef = *const c_void;
    type IOPMAssertionId = u32;
    /// `kIOPMAssertionLevelOn`.
    const IOPM_ASSERTION_LEVEL_ON: u32 = 255;
    /// The IOKit assertion-type constants are `#define`d CFString literals,
    /// not exported symbols: `kIOPMAssertionTypePreventUserIdleSystemSleep`
    /// (idle sleep, battery and AC — `caffeinate -i`) and
    /// `kIOPMAssertionTypePreventSystemSleep` (all system sleep, lid close
    /// included, AC power only — `caffeinate -s`). User-requested sleep
    /// still wins over both.
    const ASSERTION_TYPES: [&str; 2] = ["PreventUserIdleSystemSleep", "PreventSystemSleep"];

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CFStringRef,
            assertion_level: u32,
            assertion_name: CFStringRef,
            assertion_id: *mut IOPMAssertionId,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: IOPMAssertionId) -> i32;
    }

    pub struct Assertion {
        ids: Vec<IOPMAssertionId>,
    }

    impl Assertion {
        pub fn acquire(reason: &str) -> Self {
            // CFStringRef toll-free-bridges to NSString.
            let name = NSString::from_str(reason);
            let name = (&*name as *const NSString).cast::<c_void>();
            let mut ids = Vec::new();
            for assertion_type in ASSERTION_TYPES {
                let assertion_type = NSString::from_str(assertion_type);
                let assertion_type = (&*assertion_type as *const NSString).cast::<c_void>();
                let mut id = 0;
                // SAFETY: all pointers are valid CFStringRefs for the call's
                // duration; `id` is written on success only.
                if unsafe {
                    IOPMAssertionCreateWithName(
                        assertion_type,
                        IOPM_ASSERTION_LEVEL_ON,
                        name,
                        &mut id,
                    )
                } == 0
                {
                    ids.push(id);
                } else {
                    eprintln!("could not declare a power assertion; sleep is not prevented");
                }
            }
            Self { ids }
        }
    }

    impl Drop for Assertion {
        fn drop(&mut self) {
            for id in self.ids.drain(..) {
                // SAFETY: `id` is a live assertion id this process created.
                unsafe { IOPMAssertionRelease(id) };
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use windows_sys::Win32::System::Power::{
        ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
    };

    pub struct Assertion {
        _private: (),
    }

    impl Assertion {
        pub fn acquire(_reason: &str) -> Self {
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
            }
            Self { _private: () }
        }
    }

    impl Drop for Assertion {
        fn drop(&mut self) {
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    pub struct Assertion {
        _private: (),
    }

    impl Assertion {
        pub fn acquire(_reason: &str) -> Self {
            Self { _private: () }
        }
    }
}
