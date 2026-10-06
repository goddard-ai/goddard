//! Dependency-light leaf modules extracted from `waku-core`.
//!
//! These modules sit at the bottom of the crate graph: they depend only on
//! `waku-protocol`, `waku-localization`, and platform libraries, never on the
//! provider, persistence, or daemon machinery above them. `waku-core` re-exports
//! every module so existing `waku_core::x` paths keep working.

// The catalog is embedded once by `waku-localization` and registered into
// `waku-protocol` when linked; `crate::i18n` re-exports it.
#[macro_export]
macro_rules! tr {
    ($key:expr) => {
        ::waku_localization::translate($key)
    };
    ($key:expr, $($name:ident => $value:expr),+ $(,)?) => {
        ::waku_localization::translate_args(
            $key,
            &[$( (stringify!($name), $value.to_string()) ),+],
        )
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        ::waku_localization::translate_args(
            $key,
            &[$( (stringify!($name), $value.to_string()) ),+],
        )
    };
}

/// Pair a translated fallback string with its `WireTranslation` so wire
/// emitters ship the semantic and each client renders its own locale.
/// Args are recorded by name so the client can substitute `%{name}` itself.
/// A `KeyedError` whose message is the `tr!` fallback and whose key+args ride
/// to the RPC boundary so clients can render their own locale.
#[macro_export]
macro_rules! keyed {
    ($($t:tt)*) => {
        ::waku_protocol::KeyedError::localized($crate::localized!($($t)*))
    };
}

#[macro_export]
macro_rules! localized {
    ($key:expr) => {
        ($crate::tr!($key), ::waku_protocol::WireTranslation::new($key, []))
    };
    ($key:expr, $($name:ident = $value:expr),+ $(,)?) => {
        (
            $crate::tr!($key, $($name = $value),+),
            ::waku_protocol::WireTranslation::new(
                $key,
                [$( (stringify!($name), $value.to_string()) ),+],
            ),
        )
    };
}

pub mod ansi;
pub mod attachments;
pub mod blob_store;
pub mod frontmatter;
pub mod fs_ext;
pub mod http_wire;
pub mod i18n;
pub mod identity;
pub mod issue_templates;
pub mod lan;
pub mod pairing;
pub mod power;
pub mod pressure;
pub mod projectless;
pub mod protocol;
pub mod resource_broker;
pub mod settings;
pub mod subprocess;
pub mod theme;
