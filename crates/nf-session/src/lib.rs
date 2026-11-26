mod connect;
mod game;
mod peer;
mod session;
mod sync;

pub use connect::RelayAddress;
pub use game::{GameEntrance, GameEvent, serve_game};
pub use nf_relay::protocol::{CODE_LENGTH, PeerId, display_code, normalize_code};
pub use peer::{Datagram, Inbox, IncomingStream, PeerConnection};
pub use session::{HostControl, HostEvent, HostedSession, JoinedSession, host, join};
pub use sync::{FileChange, GameState, Mirror, SyncEvent, SyncHandle, guest_sync, host_sync, is_syncable};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("'{0}' is not a relay address. Use host:port, optionally followed by #fingerprint.")]
    InvalidRelay(String),
    #[error("Could not reach the relay: {0}")]
    Connect(String),
    #[error("{0}")]
    Refused(String),
    #[error("{0}")]
    Ended(String),
    #[error("{0}")]
    Sync(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
