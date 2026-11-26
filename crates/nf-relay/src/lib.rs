pub mod identity;
pub mod protocol;
mod server;

use std::time::Duration;

pub use identity::{Identity, fingerprint_of};
pub use server::{RelayEvent, RelayServer, deliver_datagram};

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("The relay identity could not be prepared: {0}")]
    Identity(String),
    #[error("The relay could not listen: {0}")]
    Bind(std::io::Error),
}

pub fn transport_config() -> quinn::TransportConfig {
    let mut config = quinn::TransportConfig::default();
    config.keep_alive_interval(Some(Duration::from_secs(5)));
    config.max_idle_timeout(Some(quinn::VarInt::from_u32(30_000).into()));
    config.max_concurrent_bidi_streams(1024u32.into());
    config.max_concurrent_uni_streams(1024u32.into());
    config
}
