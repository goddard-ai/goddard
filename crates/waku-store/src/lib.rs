//! Persistence, data-directory migration, and Boss context snapshots for Goddard.
//! `waku-core` re-exports these modules to preserve existing paths.

pub use waku_base::settings::DaemonSettings;
pub(crate) use waku_base::{attachments, blob_store, i18n, identity, theme};
pub(crate) use waku_git::checkpoint;
pub(crate) use waku_protocol::model;

pub mod boss_context;
pub mod migration;
pub mod persistence;
