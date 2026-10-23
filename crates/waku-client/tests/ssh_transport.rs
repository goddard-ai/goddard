#![cfg(unix)]

// Exercise the desktop's real SSH module without linking the GPUI app. The
// packaged-binary lookup is an external boundary; fixtures supply that asset.
mod daemon {
    pub fn daemon_executable_path() -> anyhow::Result<std::path::PathBuf> {
        std::env::var_os("GODDARD_SSH_TEST_DAEMON")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("no packaged daemon in this fixture"))
    }
}

#[allow(dead_code)]
#[path = "../../../src/ssh.rs"]
mod ssh;
