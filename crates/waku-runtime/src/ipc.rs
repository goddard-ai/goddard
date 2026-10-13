//! Private local transport: length-prefixed JSON frames over Unix sockets.
//!
//! The endpoint lives inside the namespace root, which is owner-only — the
//! socket path is the discovery proof and the kernel's peer credentials
//! verify the caller is the same OS user. PID alone is never sufficient
//! evidence of a runtime instance; `endpoint.json` carries the live
//! instance so a connecting controller can tell replacement from the
//! process it expected.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// One frame carries one JSON message. The bound is a transport integrity
/// check — the host's own budgets cap semantic payloads far lower, and a
/// frame this size can never be a legitimate control message.
pub const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;

const ENDPOINT_NAME: &str = "endpoint.json";

/// The socket path for a namespace root. Real namespace roots sit deep
/// under application directories — far past `sun_path` — so the socket
/// lives in the temp dir named by a stable digest of the root. Discovery
/// still flows through `endpoint.json` inside the owner-only root, and
/// the socket itself is owner-only: the hash is routing, not authority.
pub fn socket_path(root: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    let digest = format!("{:016x}", hasher.finish());
    let base = std::env::temp_dir();
    let candidate = base.join(format!("waku-rt-{digest}.sock"));
    if candidate.as_os_str().len() <= 100 {
        candidate
    } else {
        PathBuf::from("/tmp").join(format!("waku-rt-{digest}.sock"))
    }
}

/// Write one JSON frame: `u32` length + payload.
pub fn send<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(io::Error::other)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame exceeds the maximum size",
        ));
    }
    stream.write_all(&(payload.len() as u32).to_be_bytes())?;
    stream.write_all(&payload)
}

/// Read one frame; `Ok(None)` on clean EOF. Oversize, torn, and malformed
/// frames are errors — a peer that cannot frame is not speaking the
/// protocol. A timeout is only retriable before any bytes of the next
/// frame arrive: once a frame is partially consumed the stream can never
/// realign, so a torn read is a fatal protocol error, not a retry.
pub fn recv<T: DeserializeOwned>(stream: &mut UnixStream) -> io::Result<Option<T>> {
    let mut len = [0u8; 4];
    match read_frame_part(stream, &mut len, true)? {
        PartRead::Complete => {}
        PartRead::CleanEof => return Ok(None),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds the maximum size"),
        ));
    }
    let mut payload = vec![0u8; len];
    read_frame_part(stream, &mut payload, false)?;
    serde_json::from_slice(&payload)
        .map(Some)
        .map_err(io::Error::other)
}

enum PartRead {
    Complete,
    CleanEof,
}

/// Fill `buf` or explain why not. A retryable timeout is only possible
/// when nothing of this part was consumed; a part-consumed buffer is a
/// torn frame the stream cannot recover from.
fn read_frame_part(stream: &mut UnixStream, buf: &mut [u8], eof_ok: bool) -> io::Result<PartRead> {
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => {
                if eof_ok && filled == 0 {
                    return Ok(PartRead::CleanEof);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "peer closed mid-frame",
                ));
            }
            Ok(n) => filled += n,
            Err(e)
                if eof_ok
                    && filled == 0
                    && (e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut) =>
            {
                return Err(e);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("frame torn after {filled} bytes: {e}"),
                ));
            }
        }
    }
    Ok(PartRead::Complete)
}

/// The discovery record written beside the socket. It proves namespace and
/// instance identity for callers; the OS-held lock inside the same root is
/// the singleton proof.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub namespace: String,
    pub instance: Uuid,
    pub socket: String,
    pub pid: u32,
    #[serde(default)]
    pub emergency_socket: Option<String>,
}

impl Endpoint {
    pub fn path(root: &Path) -> PathBuf {
        root.join(ENDPOINT_NAME)
    }

    /// Publish the endpoint. Owner-only like the identity file — it names
    /// the live instance and must not be world-readable metadata a foreign
    /// process could impersonate.
    pub fn write(root: &Path, namespace: &str, instance: Uuid) -> io::Result<()> {
        let endpoint = Self {
            namespace: namespace.to_string(),
            instance,
            socket: socket_path(root).to_string_lossy().into_owned(),
            pid: std::process::id(),
            emergency_socket: Some(emergency_socket_path(root).to_string_lossy().into_owned()),
        };
        let raw = serde_json::to_vec_pretty(&endpoint).map_err(io::Error::other)?;
        use std::os::unix::fs::OpenOptionsExt;
        let temporary = root.join(format!("endpoint-{}", Uuid::new_v4()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&raw)?;
        file.sync_all()?;
        fs::rename(temporary, Self::path(root))?;
        fs::File::open(root)?.sync_all()?;
        Ok(())
    }

    /// Load and sanity-check an endpoint record.
    pub fn load(root: &Path) -> io::Result<Self> {
        private_root(root).map_err(io::Error::other)?;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(Self::path(root))?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.len() > 16 * 1024
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid endpoint ownership",
            ));
        }
        let raw = fs::read(Self::path(root))?;
        serde_json::from_slice(&raw).map_err(io::Error::other)
    }
}

/// Bind the runtime's private listener inside its namespace root. A stale
/// socket file is removed — the namespace lock is the singleton proof, so
/// only a dead owner could have left it behind.
pub fn emergency_socket_path(root: &Path) -> PathBuf {
    socket_path(root).with_extension("emergency.sock")
}

pub fn bind(root: &Path) -> io::Result<UnixListener> {
    bind_path(&socket_path(root))
}
pub fn bind_emergency(root: &Path) -> io::Result<UnixListener> {
    bind_path(&emergency_socket_path(root))
}
fn bind_path(path: &Path) -> io::Result<UnixListener> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() } =>
        {
            fs::remove_file(path)?
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "foreign socket path",
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Connect to the runtime serving a namespace root.
pub fn connect(root: &Path) -> io::Result<UnixStream> {
    UnixStream::connect(socket_path(root))
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0 {
        Ok(uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if rc == 0 {
        Ok(cred.uid)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Whether the peer on this connection is the same OS user. Anything less
/// is not a private runtime channel regardless of the credentials it then
/// presents.
pub fn same_user(stream: &UnixStream) -> io::Result<bool> {
    Ok(peer_uid(stream)? == unsafe { libc::geteuid() })
}

/// Create or verify a current-user-owned, mode-0700 state root.
pub fn private_root(root: &Path) -> Result<(), crate::HostError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder
        .create(root)
        .map_err(|e| crate::HostError::Io(e.to_string()))?;
    let metadata = fs::symlink_metadata(root).map_err(|e| crate::HostError::Io(e.to_string()))?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(crate::HostError::Unauthorized);
    }
    Ok(())
}
