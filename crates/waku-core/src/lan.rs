//! `_waku._tcp` DNS-SD advertisement, live only while the daemon listens
//! on a non-loopback address. Browsers and the mobile app — clients that
//! can't run an iroh endpoint — discover the daemon through ordinary
//! Bonjour; iroh-capable clients use `waku-share`'s own multicast lookup.
//! The record carries metadata only: instance name, protocol version, and
//! the share endpoint's id so a client can confirm both discoveries point
//! at the same install. The token is never advertised.

use std::collections::HashMap;

use anyhow::Context as _;

/// The DNS-SD service type every exposed daemon registers.
pub const SERVICE_TYPE: &str = "_waku._tcp.local.";

/// A live `_waku._tcp` registration. Dropping unregisters and stops the
/// responder; `serve` holds it for the daemon's lifetime.
pub struct LanAdvert {
    daemon: mdns_sd::ServiceDaemon,
    fullname: String,
}

impl LanAdvert {
    /// Announce `name` as a waku daemon on `port`. `instance_id` is the
    /// share endpoint's id — the same identity friend codes and iroh LAN
    /// discovery carry, so clients can correlate the two paths.
    pub fn start(
        name: &str,
        instance_id: &str,
        port: u16,
        protocol_version: u32,
    ) -> anyhow::Result<Self> {
        let daemon = mdns_sd::ServiceDaemon::new().context("starting mDNS responder")?;
        let properties = HashMap::from([
            // Advert format version — bump if TXT keys ever change shape.
            ("v".to_string(), "1".to_string()),
            ("pv".to_string(), protocol_version.to_string()),
            ("id".to_string(), instance_id.to_string()),
            ("name".to_string(), name.to_string()),
        ]);
        // A synthetic hostname keeps the SRV target unique per install —
        // collisions would merge address records across machines.
        let short_id: String = instance_id.chars().take(8).collect();
        let host_name = format!("waku-{short_id}.local.");
        let service =
            mdns_sd::ServiceInfo::new(SERVICE_TYPE, name, &host_name, "", port, properties)
                .context("invalid LAN service info")?
                .enable_addr_auto();
        let fullname = service.get_fullname().to_string();
        daemon
            .register(service)
            .context("registering waku daemon on the LAN")?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for LanAdvert {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}
