//! Local identity: a persistent iroh secret key and the friend code that
//! advertises it. The keypair lives next to Goddard's other state
//! (`~/.goddard/` in release) with owner-only permissions.

use std::path::Path;

use anyhow::{Context as _, bail};
use iroh::{EndpointId, SecretKey};

const KEY_FILENAME: &str = "friends-identity.key";
pub const FRIEND_CODE_PREFIX: &str = "gfr-";

/// Load the keypair from `dir`, generating one on first run.
pub fn load_or_create(dir: &Path) -> anyhow::Result<SecretKey> {
    let path = dir.join(KEY_FILENAME);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("corrupt identity key at {}", path.display()))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = SecretKey::generate();
            let tmp = path.with_extension("tmp");
            std::fs::write(&tmp, key.to_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
            }
            std::fs::rename(&tmp, &path)?;
            Ok(key)
        }
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// `gfr-<base32 node id>` — what the user copies to a friend.
pub fn friend_code(node: EndpointId) -> String {
    format!("{FRIEND_CODE_PREFIX}{node}")
}

/// Parse a friend code back into a dialable node id. Accepts the bare
/// node id too so pasting a sendme-style id still works.
pub fn parse_friend_code(code: &str) -> anyhow::Result<EndpointId> {
    let trimmed = code.trim();
    let id_str = trimmed.strip_prefix(FRIEND_CODE_PREFIX).unwrap_or(trimmed);
    id_str
        .parse::<EndpointId>()
        .map_err(|_| anyhow::anyhow!("invalid friend code"))
        .and_then(|id| {
            if id_str.is_empty() {
                bail!("empty friend code");
            }
            Ok(id)
        })
}
