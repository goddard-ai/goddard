//! The daemon's opt-in link to a surviving provider-runtime host.
//!
//! One `StableLink` is the daemon-side controller for one host session: it
//! implements [`driver::DriverControl`] so the rest of the daemon — forwarder,
//! projections, transcript recording — sees the same driver surface a local
//! provider process presents, while the provider itself lives under the
//! separately surviving `waku-runtime-host` process.
//!
//! Semantics the link owes the rest of the daemon:
//!
//! - Attach, don't resume: a task whose durable link row still names a live
//!   host session reconnects to that session's existing work instead of
//!   admitting a new provider turn.
//! - Dedup before effects: a command's recorded truth (`client.command`) is
//!   consulted after transport loss rather than resending under a new
//!   identity, and forwarded agent requests answer from durable receipts.
//! - Durable before ack: the daemon only acknowledges a host event sequence
//!   after the forwarder has applied it — the marker the pump interleaves is
//!   the ack boundary, and `stable_runtime_links.applied_seq` is the
//!   crash-resume truth.
//! - Uncertain, never resent: effects the daemon cannot confirm stay
//!   `Uncertain` — no automatic resend on either side.

use std::path::{Path, PathBuf};

use crate::model::ProviderKind;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::*;

// No stable transport exists on non-Unix hosts. This uninhabited type keeps
// the shared runtime bookkeeping explicit without admitting a fake link.
#[cfg(not(unix))]
#[derive(Clone)]
pub(crate) enum StableLink {}

#[cfg(not(unix))]
impl StableLink {
    pub(crate) fn note_applied(&self, _cursor: &waku_protocol::model::RuntimeEventCursor) {
        match *self {}
    }
}

/// The namespace the Goddard daemon's controller owns on the host.
pub(crate) const STABLE_NAMESPACE: &str = "goddard";

/// Env override naming the namespace root — the explicit Stage-2C opt-in.
const ROOT_ENV: &str = "WAKU_STABLE_RUNTIME_ROOT";
/// Env override listing providers routed through the host — `all` or a
/// comma-separated provider id list. Defaults to every provider when the
/// root opt-in is set.
const PROVIDERS_ENV: &str = "WAKU_STABLE_RUNTIME_PROVIDERS";
/// The default runtime namespace root under the daemon's data directory.
pub(crate) fn default_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join(STABLE_NAMESPACE)
}

/// Whether this launch should ride the stable runtime. The env root opts in
/// explicitly; a provider list without a root uses the default namespace
/// under the daemon's data dir. Provider gating defaults to every provider
/// the host has an adapter for — an unregistered adapter fails the
/// admission loudly rather than silently falling back to a legacy driver.
pub(crate) fn enabled(
    provider: ProviderKind,
    data_dir: &Path,
    test_root: Option<&PathBuf>,
) -> Option<PathBuf> {
    let root = test_root.cloned().or_else(|| {
        std::env::var(ROOT_ENV)
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    });
    let list = std::env::var(PROVIDERS_ENV).ok();
    let allowed = match list.as_deref() {
        // A root alone opts in for every provider the host can serve.
        None => root.is_some(),
        Some(list) => list.trim() == "all" || list.split(',').any(|p| p.trim() == provider.id()),
    };
    if !allowed {
        return None;
    }
    root.or_else(|| list.is_some().then(|| default_root(data_dir)))
}
