use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use quinn::crypto::rustls::QuicServerConfig;
use quinn::{Connection, Endpoint, RecvStream, SendStream, VarInt};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;

use crate::identity::Identity;
use crate::protocol::{
    ALPN, CLOSE_KICKED, CLOSE_PROTOCOL, CLOSE_REFUSED, CLOSE_ROOM_CLOSED, ClientMessage, HOST, Header, MAX_DATAGRAM_LENGTH, PeerId, ServerMessage, generate_code,
    normalize_code, read_header, read_message, write_header, write_message,
};
use crate::{RelayError, transport_config};

const ADMISSION_TIMEOUT: Duration = Duration::from_secs(120);

struct Member {
    name: String,
    connection: Connection,
    control: UnboundedSender<ServerMessage>,
}

struct Pending {
    decision: oneshot::Sender<Result<(), String>>,
}

struct Room {
    host: Member,
    guests: HashMap<PeerId, Member>,
    pending: HashMap<PeerId, Pending>,
    next_peer: PeerId,
}

#[derive(Default)]
struct State {
    rooms: HashMap<String, Room>,
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<State>>);

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn route(&self, code: &str, from: PeerId, to: PeerId) -> Option<Connection> {
        let state = self.lock();
        let room = state.rooms.get(code)?;
        if from == HOST {
            room.guests.get(&to).map(|guest| guest.connection.clone())
        } else if to == HOST {
            Some(room.host.connection.clone())
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayEvent {
    RoomOpened { code: String, host: String, address: SocketAddr },
    RoomClosed { code: String },
    GuestRequested { code: String, guest: PeerId, name: String },
    GuestJoined { code: String, guest: PeerId },
    GuestLeft { code: String, guest: PeerId },
}

impl std::fmt::Display for RelayEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoomOpened { code, host, address } => write!(formatter, "room {code} opened by {host} ({address})"),
            Self::RoomClosed { code } => write!(formatter, "room {code} closed"),
            Self::GuestRequested { code, guest, name } => write!(formatter, "room {code}: {name} asks to join as guest {guest}"),
            Self::GuestJoined { code, guest } => write!(formatter, "room {code}: guest {guest} joined"),
            Self::GuestLeft { code, guest } => write!(formatter, "room {code}: guest {guest} left"),
        }
    }
}

pub struct RelayServer {
    endpoint: Endpoint,
    shared: Shared,
}

impl RelayServer {
    pub fn bind(listen: SocketAddr, identity: &Identity) -> Result<Self, RelayError> {
        let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .and_then(|builder| builder.with_no_client_auth().with_single_cert(identity.chain.clone(), identity.key.clone_key()))
            .map_err(|error| RelayError::Identity(error.to_string()))?;
        tls.alpn_protocols = vec![ALPN.to_vec()];
        let crypto = QuicServerConfig::try_from(tls).map_err(|error| RelayError::Identity(error.to_string()))?;
        let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
        config.transport_config(Arc::new(transport_config()));
        let endpoint = Endpoint::server(config, listen).map_err(RelayError::Bind)?;
        Ok(Self {
            endpoint,
            shared: Shared::default(),
        })
    }

    pub fn local_address(&self) -> SocketAddr {
        self.endpoint.local_addr().unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)))
    }

    pub async fn run(self, events: UnboundedSender<RelayEvent>) {
        while let Some(incoming) = self.endpoint.accept().await {
            let shared = self.shared.clone();
            let events = events.clone();
            tokio::spawn(async move {
                if let Ok(connection) = incoming.await {
                    welcome(connection, shared, events).await;
                }
            });
        }
    }
}

async fn write_control(mut stream: SendStream, mut messages: UnboundedReceiver<ServerMessage>) {
    while let Some(message) = messages.recv().await {
        if write_message(&mut stream, &message).await.is_err() {
            return;
        }
    }
    let _ = stream.finish();
}

fn close(connection: &Connection, code: u32, reason: &str) {
    connection.close(VarInt::from_u32(code), reason.as_bytes());
}

async fn welcome(connection: Connection, shared: Shared, events: UnboundedSender<RelayEvent>) {
    let Ok((send, mut receive)) = connection.accept_bi().await else {
        return;
    };
    let first = match read_message::<ClientMessage>(&mut receive).await {
        Ok(Some(message)) => message,
        _ => return close(&connection, CLOSE_PROTOCOL, "Expected a session request."),
    };
    let (control, outgoing) = mpsc::unbounded_channel();
    tokio::spawn(write_control(send, outgoing));
    let member = |name: String| Member {
        name,
        connection: connection.clone(),
        control: control.clone(),
    };
    match first {
        ClientMessage::CreateRoom { name } => host_room(member(name), receive, shared, events).await,
        ClientMessage::JoinRoom { code, name } => join_room(member(name), normalize_code(&code), shared, events).await,
        _ => close(&connection, CLOSE_PROTOCOL, "Expected a session request."),
    }
}

async fn host_room(host: Member, mut receive: RecvStream, shared: Shared, events: UnboundedSender<RelayEvent>) {
    let connection = host.connection.clone();
    let control = host.control.clone();
    let host_name = host.name.clone();
    let code = {
        let mut state = shared.lock();
        let code = std::iter::repeat_with(generate_code).find(|code| !state.rooms.contains_key(code)).unwrap_or_default();
        state.rooms.insert(
            code.clone(),
            Room {
                host,
                guests: HashMap::new(),
                pending: HashMap::new(),
                next_peer: HOST + 1,
            },
        );
        code
    };
    let _ = control.send(ServerMessage::RoomCreated { code: code.clone() });
    let _ = events.send(RelayEvent::RoomOpened {
        code: code.clone(),
        host: host_name,
        address: connection.remote_address(),
    });
    tokio::select! {
        _ = host_commands(&mut receive, &shared, &code, &events) => {}
        _ = forward_everything(&connection, &shared, &code, HOST) => {}
    }
    let room = shared.lock().rooms.remove(&code);
    if let Some(room) = room {
        for guest in room.guests.values() {
            close(&guest.connection, CLOSE_ROOM_CLOSED, "The host ended the session.");
        }
        for pending in room.pending.into_values() {
            let _ = pending.decision.send(Err("The host ended the session.".to_owned()));
        }
    }
    close(&connection, CLOSE_ROOM_CLOSED, "The session ended.");
    let _ = events.send(RelayEvent::RoomClosed { code });
}

async fn host_commands(receive: &mut RecvStream, shared: &Shared, code: &str, events: &UnboundedSender<RelayEvent>) {
    while let Ok(Some(message)) = read_message::<ClientMessage>(receive).await {
        let mut state = shared.lock();
        let Some(room) = state.rooms.get_mut(code) else {
            return;
        };
        match message {
            ClientMessage::Admit { guest } => {
                if let Some(pending) = room.pending.remove(&guest) {
                    let _ = pending.decision.send(Ok(()));
                }
            }
            ClientMessage::Refuse { guest, reason } => {
                if let Some(pending) = room.pending.remove(&guest) {
                    let _ = pending.decision.send(Err(reason));
                }
            }
            ClientMessage::Kick { guest } => {
                if let Some(member) = room.guests.remove(&guest) {
                    close(&member.connection, CLOSE_KICKED, "The host removed you from the session.");
                    let _ = events.send(RelayEvent::GuestLeft { code: code.to_owned(), guest });
                }
            }
            ClientMessage::CreateRoom { .. } | ClientMessage::JoinRoom { .. } => {}
        }
    }
}

async fn join_room(guest: Member, code: String, shared: Shared, events: UnboundedSender<RelayEvent>) {
    let connection = guest.connection.clone();
    let (decision, decided) = oneshot::channel();
    let registered = {
        let mut state = shared.lock();
        state.rooms.get_mut(&code).map(|room| {
            let id = room.next_peer;
            room.next_peer += 1;
            room.pending.insert(id, Pending { decision });
            let _ = room.host.control.send(ServerMessage::JoinRequested { guest: id, name: guest.name.clone() });
            (id, room.host.name.clone())
        })
    };
    let Some((id, host_name)) = registered else {
        return close(&connection, CLOSE_REFUSED, "There is no session with this code.");
    };
    let _ = events.send(RelayEvent::GuestRequested {
        code: code.clone(),
        guest: id,
        name: guest.name.clone(),
    });
    let outcome = tokio::select! {
        decided = tokio::time::timeout(ADMISSION_TIMEOUT, decided) => match decided {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => Err("The host ended the session.".to_owned()),
            Err(_) => Err("The host did not answer in time.".to_owned()),
        },
        _ = connection.closed() => Err(String::new()),
    };
    if let Err(reason) = outcome {
        let mut state = shared.lock();
        if let Some(room) = state.rooms.get_mut(&code) {
            room.pending.remove(&id);
            let _ = room.host.control.send(ServerMessage::GuestLeft { guest: id });
        }
        return close(&connection, CLOSE_REFUSED, &reason);
    }
    let control = guest.control.clone();
    let admitted = {
        let mut state = shared.lock();
        state.rooms.get_mut(&code).map(|room| room.guests.insert(id, guest)).is_some()
    };
    if !admitted {
        return close(&connection, CLOSE_ROOM_CLOSED, "The host ended the session.");
    }
    let _ = control.send(ServerMessage::Joined { id, host: host_name });
    let _ = events.send(RelayEvent::GuestJoined { code: code.clone(), guest: id });
    forward_everything(&connection, &shared, &code, id).await;
    let mut state = shared.lock();
    if let Some(room) = state.rooms.get_mut(&code)
        && room.guests.remove(&id).is_some()
    {
        let _ = room.host.control.send(ServerMessage::GuestLeft { guest: id });
        let _ = events.send(RelayEvent::GuestLeft { code, guest: id });
    }
}

async fn forward_everything(connection: &Connection, shared: &Shared, code: &str, me: PeerId) {
    tokio::select! {
        _ = forward_bidirectional_streams(connection, shared, code, me) => {}
        _ = forward_unidirectional_streams(connection, shared, code, me) => {}
        _ = forward_datagrams(connection, shared, code, me) => {}
    }
}

async fn forward_bidirectional_streams(connection: &Connection, shared: &Shared, code: &str, me: PeerId) {
    while let Ok((send, receive)) = connection.accept_bi().await {
        let shared = shared.clone();
        let code = code.to_owned();
        tokio::spawn(async move {
            let _ = splice_bidirectional(send, receive, &shared, &code, me).await;
        });
    }
}

async fn splice_bidirectional(mut send: SendStream, mut receive: RecvStream, shared: &Shared, code: &str, me: PeerId) -> std::io::Result<()> {
    let header = read_header(&mut receive).await?;
    let Some(target) = shared.route(code, me, header.peer) else {
        let _ = send.reset(VarInt::from_u32(CLOSE_PROTOCOL));
        return Ok(());
    };
    let (mut target_send, mut target_receive) = target.open_bi().await?;
    write_header(&mut target_send, Header::new(me, header.channel)).await?;
    let forward = async {
        tokio::io::copy(&mut receive, &mut target_send).await?;
        target_send.shutdown().await
    };
    let backward = async {
        tokio::io::copy(&mut target_receive, &mut send).await?;
        send.shutdown().await
    };
    tokio::try_join!(forward, backward).map(|_| ())
}

async fn forward_unidirectional_streams(connection: &Connection, shared: &Shared, code: &str, me: PeerId) {
    while let Ok(mut receive) = connection.accept_uni().await {
        let shared = shared.clone();
        let code = code.to_owned();
        tokio::spawn(async move {
            let header = read_header(&mut receive).await?;
            let Some(target) = shared.route(&code, me, header.peer) else {
                return Ok(());
            };
            let mut target_send = target.open_uni().await?;
            write_header(&mut target_send, Header::new(me, header.channel)).await?;
            tokio::io::copy(&mut receive, &mut target_send).await?;
            target_send.shutdown().await
        });
    }
}

pub fn deliver_datagram(target: &Connection, header: Header, payload: &[u8]) {
    let framed = header.frame(payload);
    if target.max_datagram_size().is_some_and(|limit| framed.len() <= limit) {
        let _ = target.send_datagram(framed.into());
        return;
    }
    if payload.len() > MAX_DATAGRAM_LENGTH {
        return;
    }
    let target = target.clone();
    tokio::spawn(async move {
        let mut stream = target.open_uni().await?;
        stream.write_all(&framed).await?;
        stream.shutdown().await
    });
}

async fn forward_datagrams(connection: &Connection, shared: &Shared, code: &str, me: PeerId) {
    let mut routes: HashMap<PeerId, Connection> = HashMap::new();
    while let Ok(datagram) = connection.read_datagram().await {
        let Some((header, payload)) = Header::decode(&datagram) else {
            continue;
        };
        let target = match routes.get(&header.peer) {
            Some(target) if target.close_reason().is_none() => target.clone(),
            _ => {
                let Some(target) = shared.route(code, me, header.peer) else {
                    continue;
                };
                routes.insert(header.peer, target.clone());
                target
            }
        };
        deliver_datagram(&target, Header::new(me, header.channel), payload);
    }
}
