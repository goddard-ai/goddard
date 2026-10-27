//! Serialize descriptor publication and retirement across daemon processes.
//! Operations stay relative to the opened directory: replacing that directory
//! cannot redirect an older holder's mutation into a successor's directory.

use super::*;
use std::ffi::{CStr, CString};
use std::fs::{File, TryLockError};
use std::io::{Read as _, Write as _};
use std::os::fd::{AsRawFd as _, FromRawFd as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};

pub(super) struct LockedDescriptor {
    _lock: File,
    directory: File,
    name: CString,
    path: PathBuf,
}

impl LockedDescriptor {
    /// Worker-only. Contention returns a retryable error after one second;
    /// an abandoned process releases its OS lock without a stale-lock repair.
    pub(super) fn acquire(path: &Path) -> anyhow::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("ingress directory is missing"))?;
        crate::fs_ext::create_private_dir_all(parent)?;
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK)
            .open(parent)?;
        verify_private(&directory.metadata()?, true)?;
        let name = CString::new(
            path.file_name()
                .ok_or_else(|| anyhow!("ingress filename is missing"))?
                .as_bytes(),
        )?;
        let lock_name = CString::new([name.as_bytes(), b".lock"].concat())?;
        let lock = open_at(&directory, &lock_name, libc::O_RDWR | libc::O_CREAT, 0o600)?;
        verify_private(&lock.metadata()?, false)?;
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "agent ingress is being updated; retry retirement or attachment",
                    )
                    .into());
                }
                Err(TryLockError::Error(error)) => return Err(error.into()),
            }
        }
        Ok(Self {
            _lock: lock,
            directory,
            name,
            path: path.to_owned(),
        })
    }

    pub(super) fn read(&self) -> anyhow::Result<Option<IngressDescriptor>> {
        let file = match open_at(&self.directory, &self.name, libc::O_RDONLY, 0) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        verify_private(&metadata, false)?;
        anyhow::ensure!(
            metadata.len() <= 64 * 1024,
            "ingress descriptor is oversized"
        );
        let mut bytes = Vec::new();
        file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len() <= 64 * 1024, "ingress descriptor is oversized");
        serde_json::from_slice(&bytes)
            .map(Some)
            .context("ingress descriptor is invalid")
    }

    pub(super) fn publish(&mut self, descriptor: &IngressDescriptor) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(descriptor)?;
        anyhow::ensure!(bytes.len() <= 64 * 1024, "ingress descriptor is oversized");
        let temporary = CString::new(format!("ingress-{}.tmp", Uuid::new_v4()))?;
        // Only remove a staging file after this call successfully created it.
        let mut file = open_at(
            &self.directory,
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        let staged = (|| -> std::io::Result<()> {
            file.write_all(&bytes)?;
            file.sync_all()?;
            Ok(())
        })();
        drop(file);
        if let Err(error) = staged {
            let _ = unlink_at(&self.directory, &temporary);
            return Err(error.into());
        }
        // SAFETY: the directory fd and both NUL-terminated names remain live.
        let renamed = unsafe {
            libc::renameat(
                self.directory.as_raw_fd(),
                temporary.as_ptr(),
                self.directory.as_raw_fd(),
                self.name.as_ptr(),
            )
        };
        if renamed != 0 {
            let error = std::io::Error::last_os_error();
            let _ = unlink_at(&self.directory, &temporary);
            return Err(error.into());
        }
        self.directory
            .sync_all()
            .context("could not sync ingress publication")
    }

    /// False means the pathname belongs to a different owner and is preserved.
    /// Missing files are idempotent; uncertain reads/removals retain authority.
    pub(super) fn retire(&mut self, intent: &StableStopIntent) -> anyhow::Result<bool> {
        let expected = intent
            .descriptor_owner
            .as_ref()
            .ok_or_else(|| anyhow!("ingress owner is missing"))?;
        anyhow::ensure!(
            expected.path == self.path,
            "ingress retirement path changed"
        );
        if let Some(descriptor) = self.read()? {
            if descriptor.version != INGRESS_DESCRIPTOR_VERSION
                || descriptor.root != intent.root
                || descriptor.task_id != intent.session_id
                || descriptor.grant.session_id != intent.runtime_session_id
                || descriptor.grant.generation != intent.generation
                || descriptor.grant.grant_id != expected.grant_id
            {
                return Ok(false);
            }
            match unlink_at(&self.directory, &self.name) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.directory
            .sync_all()
            .context("could not sync ingress retirement")?;
        Ok(true)
    }
}

fn verify_private(metadata: &std::fs::Metadata, directory: bool) -> anyhow::Result<()> {
    // SAFETY: geteuid has no preconditions and reads this process's identity.
    let uid = unsafe { libc::geteuid() };
    anyhow::ensure!(
        metadata.uid() == uid
            && metadata.mode() & 0o077 == 0
            && if directory {
                metadata.is_dir()
            } else {
                metadata.is_file() && metadata.nlink() == 1
            },
        "ingress evidence must be a private owned {}",
        if directory { "directory" } else { "file" }
    );
    Ok(())
}

fn open_at(directory: &File, name: &CStr, flags: i32, mode: libc::mode_t) -> std::io::Result<File> {
    // SAFETY: fd and C string remain live; callers use a single filename, and
    // the returned nonnegative fd is transferred into exactly one File owner.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            libc::c_uint::from(mode),
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a new owned fd.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn unlink_at(directory: &File, name: &CStr) -> std::io::Result<()> {
    // SAFETY: fd and NUL-terminated filename remain live for the call.
    if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
