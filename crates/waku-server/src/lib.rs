//! WebSocket and HTTP transport, event replay, and request dispatch for Goddard.
mod sinks;
pub use sinks::*;
mod server;
pub use server::*;

pub mod idle_shutdown;

#[cfg(test)]
mod idle_shutdown_tests;
