mod datagram;
mod meter;
mod stream;

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc::UnboundedSender;

pub use datagram::is_transient;
pub use meter::TrafficMeter;

pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Traffic {
    pub sent: u64,
    pub received: u64,
}

impl fmt::Display for Traffic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} sent, {} received", format_bytes(self.sent), format_bytes(self.received))
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TunnelEvent {
    DatagramSessionOpened { client: SocketAddr, upstream: SocketAddr },
    DatagramSessionClosed { client: SocketAddr, traffic: Traffic, reason: String },
    StreamOpened { client: SocketAddr },
    StreamClosed { client: SocketAddr, traffic: Traffic, error: Option<String> },
}

impl fmt::Display for TunnelEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DatagramSessionOpened { client, upstream } => write!(formatter, "UDP {client} opened (server sees it as {upstream})"),
            Self::DatagramSessionClosed { client, traffic, reason } => write!(formatter, "UDP {client} closed after {traffic} ({reason})"),
            Self::StreamOpened { client } => write!(formatter, "TCP {client} opened"),
            Self::StreamClosed { client, traffic, error: None } => write!(formatter, "TCP {client} closed after {traffic}"),
            Self::StreamClosed { client, traffic, error: Some(error) } => write!(formatter, "TCP {client} failed after {traffic}: {error}"),
        }
    }
}

pub struct PortForward {
    datagrams: UdpSocket,
    streams: TcpListener,
    target: SocketAddr,
    idle_timeout: Duration,
}

impl PortForward {
    pub async fn bind(listen: SocketAddr, target: SocketAddr) -> io::Result<Self> {
        let streams = TcpListener::bind(listen).await?;
        let datagrams = UdpSocket::bind(streams.local_addr()?).await?;
        Ok(Self {
            datagrams,
            streams,
            target,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
        })
    }

    pub fn with_idle_timeout(mut self, idle_timeout: Duration) -> Self {
        self.idle_timeout = idle_timeout;
        self
    }

    pub fn local_address(&self) -> io::Result<SocketAddr> {
        self.streams.local_addr()
    }

    pub async fn run(self, events: UnboundedSender<TunnelEvent>) -> io::Result<()> {
        let datagrams = datagram::relay(Arc::new(self.datagrams), self.target, self.idle_timeout, events.clone());
        let streams = stream::relay(self.streams, self.target, events);
        tokio::try_join!(datagrams, streams).map(|_| ())
    }
}

#[cfg(test)]
mod tests;
