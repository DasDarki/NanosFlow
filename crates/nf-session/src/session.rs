use nf_relay::protocol::{ClientMessage, PeerId, ServerMessage, read_message, write_message};
use quinn::{Connection, RecvStream};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::connect::{RelayAddress, connect};
use crate::peer::{Inbox, PeerConnection, describe_close, start_inbox};
use crate::SessionError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    JoinRequested { guest: PeerId, name: String },
    GuestLeft { guest: PeerId },
    Ended { reason: String },
}

#[derive(Clone)]
pub struct HostControl {
    commands: UnboundedSender<ClientMessage>,
}

impl HostControl {
    pub fn admit(&self, guest: PeerId) {
        let _ = self.commands.send(ClientMessage::Admit { guest });
    }

    pub fn refuse(&self, guest: PeerId, reason: &str) {
        let _ = self.commands.send(ClientMessage::Refuse { guest, reason: reason.to_owned() });
    }

    pub fn kick(&self, guest: PeerId) {
        let _ = self.commands.send(ClientMessage::Kick { guest });
    }
}

pub struct HostedSession {
    pub code: String,
    pub peer: PeerConnection,
    pub control: HostControl,
    pub events: UnboundedReceiver<HostEvent>,
    pub inbox: Inbox,
}

pub struct JoinedSession {
    pub id: PeerId,
    pub host_name: String,
    pub peer: PeerConnection,
    pub inbox: Inbox,
}

async fn rejection(connection: &Connection) -> SessionError {
    let error = connection.closed().await;
    SessionError::Refused(describe_close(&error))
}

async fn relay_events(mut receive: RecvStream, connection: Connection, events: UnboundedSender<HostEvent>) {
    while let Ok(Some(message)) = read_message::<ServerMessage>(&mut receive).await {
        let event = match message {
            ServerMessage::JoinRequested { guest, name } => HostEvent::JoinRequested { guest, name },
            ServerMessage::GuestLeft { guest } => HostEvent::GuestLeft { guest },
            ServerMessage::RoomCreated { .. } | ServerMessage::Joined { .. } => continue,
        };
        let _ = events.send(event);
    }
    let reason = describe_close(&connection.closed().await);
    let _ = events.send(HostEvent::Ended { reason });
}

pub async fn host(relay: &RelayAddress, name: &str) -> Result<HostedSession, SessionError> {
    let (endpoint, connection) = connect(relay).await?;
    let (mut send, mut receive) = connection.open_bi().await.map_err(|error| SessionError::Connect(describe_close(&error)))?;
    write_message(&mut send, &ClientMessage::CreateRoom { name: name.to_owned() }).await?;
    let code = match read_message::<ServerMessage>(&mut receive).await {
        Ok(Some(ServerMessage::RoomCreated { code })) => code,
        _ => return Err(rejection(&connection).await),
    };
    let inbox = start_inbox(&connection);
    let (commands, mut pending_commands) = mpsc::unbounded_channel::<ClientMessage>();
    tokio::spawn(async move {
        while let Some(command) = pending_commands.recv().await {
            if write_message(&mut send, &command).await.is_err() {
                return;
            }
        }
    });
    let (events, received_events) = mpsc::unbounded_channel();
    tokio::spawn(relay_events(receive, connection.clone(), events));
    Ok(HostedSession {
        code,
        peer: PeerConnection::new(endpoint, connection),
        control: HostControl { commands },
        events: received_events,
        inbox,
    })
}

pub async fn join(relay: &RelayAddress, code: &str, name: &str) -> Result<JoinedSession, SessionError> {
    let (endpoint, connection) = connect(relay).await?;
    let (mut send, mut receive) = connection.open_bi().await.map_err(|error| SessionError::Connect(describe_close(&error)))?;
    write_message(&mut send, &ClientMessage::JoinRoom { code: code.to_owned(), name: name.to_owned() }).await?;
    let inbox = start_inbox(&connection);
    match read_message::<ServerMessage>(&mut receive).await {
        Ok(Some(ServerMessage::Joined { id, host })) => Ok(JoinedSession {
            id,
            host_name: host,
            peer: PeerConnection::new(endpoint, connection),
            inbox,
        }),
        _ => Err(rejection(&connection).await),
    }
}
