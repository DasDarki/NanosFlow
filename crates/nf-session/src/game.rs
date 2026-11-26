use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use nf_relay::protocol::{Channel, HOST, PeerId};
use nf_tunnel::{Traffic, TrafficMeter, is_transient};
use quinn::{RecvStream, SendStream};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::timeout;

use crate::peer::{Datagram, IncomingStream, PeerConnection};

const MAX_DATAGRAM: usize = 65_535;
const SESSION_ID_LENGTH: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    PlayerOpened { peer: PeerId, session: u32 },
    PlayerClosed { peer: PeerId, session: u32, traffic: Traffic, reason: String },
    DownloadOpened { peer: PeerId },
    DownloadClosed { peer: PeerId, traffic: Traffic, error: Option<String> },
}

impl fmt::Display for GameEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlayerOpened { peer, session } => write!(formatter, "UDP session {session} of peer {peer} opened"),
            Self::PlayerClosed { peer, session, traffic, reason } => write!(formatter, "UDP session {session} of peer {peer} closed after {traffic} ({reason})"),
            Self::DownloadOpened { peer } => write!(formatter, "TCP connection of peer {peer} opened"),
            Self::DownloadClosed { peer, traffic, error: None } => write!(formatter, "TCP connection of peer {peer} closed after {traffic}"),
            Self::DownloadClosed { peer, traffic, error: Some(error) } => write!(formatter, "TCP connection of peer {peer} failed after {traffic}: {error}"),
        }
    }
}

fn frame(session: u32, payload: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(SESSION_ID_LENGTH + payload.len());
    framed.extend_from_slice(&session.to_be_bytes());
    framed.extend_from_slice(payload);
    framed
}

fn unframe(datagram: &[u8]) -> Option<(u32, &[u8])> {
    let (session, payload) = datagram.split_at_checked(SESSION_ID_LENGTH)?;
    Some((u32::from_be_bytes([session[0], session[1], session[2], session[3]]), payload))
}

async fn splice(tcp: TcpStream, mut send: SendStream, mut receive: RecvStream) -> (Traffic, Option<String>) {
    let _ = tcp.set_nodelay(true);
    let (mut tcp_read, mut tcp_write) = tcp.into_split();
    let upload = async {
        let sent = tokio::io::copy(&mut tcp_read, &mut send).await?;
        send.shutdown().await?;
        Ok::<_, io::Error>(sent)
    };
    let download = async {
        let received = tokio::io::copy(&mut receive, &mut tcp_write).await?;
        tcp_write.shutdown().await?;
        Ok::<_, io::Error>(received)
    };
    match tokio::try_join!(upload, download) {
        Ok((sent, received)) => (Traffic { sent, received }, None),
        Err(error) => (Traffic::default(), Some(error.to_string())),
    }
}

struct Upstream {
    socket: UdpSocket,
    meter: TrafficMeter,
}

async fn open_upstream(server: SocketAddr) -> io::Result<Upstream> {
    let unspecified: SocketAddr = if server.is_ipv4() { (Ipv4Addr::UNSPECIFIED, 0).into() } else { (Ipv6Addr::UNSPECIFIED, 0).into() };
    let socket = UdpSocket::bind(unspecified).await?;
    socket.connect(server).await?;
    Ok(Upstream {
        socket,
        meter: TrafficMeter::default(),
    })
}

async fn pump_server_replies(peer: PeerConnection, guest: PeerId, session: u32, upstream: Arc<Upstream>, idle_timeout: Duration) -> String {
    let mut buffer = vec![0; MAX_DATAGRAM];
    let check_interval = idle_timeout.min(Duration::from_secs(1));
    loop {
        match timeout(check_interval, upstream.socket.recv(&mut buffer)).await {
            Ok(Ok(length)) => {
                upstream.meter.add_received(length);
                peer.send_datagram(guest, Channel::GameDatagram, &frame(session, &buffer[..length]));
            }
            Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => return "the test server is not running".to_owned(),
            Ok(Err(error)) if is_transient(&error) => {}
            Ok(Err(error)) => return format!("server socket failed: {error}"),
            Err(_) if upstream.meter.idle_for() >= idle_timeout => return "idle".to_owned(),
            Err(_) => {}
        }
    }
}

pub async fn serve_game(
    peer: PeerConnection,
    mut datagrams: UnboundedReceiver<Datagram>,
    mut streams: UnboundedReceiver<IncomingStream>,
    server: SocketAddr,
    idle_timeout: Duration,
    events: UnboundedSender<GameEvent>,
) {
    let mut sessions: HashMap<(PeerId, u32), Arc<Upstream>> = HashMap::new();
    let (closed_sender, mut closed) = mpsc::unbounded_channel::<((PeerId, u32), String)>();
    loop {
        tokio::select! {
            datagram = datagrams.recv() => {
                let Some(datagram) = datagram else { return };
                let Some((session, payload)) = unframe(&datagram.payload) else { continue };
                let key = (datagram.peer, session);
                let upstream = match sessions.get(&key) {
                    Some(upstream) => upstream.clone(),
                    None => {
                        let Ok(upstream) = open_upstream(server).await else { continue };
                        let upstream = Arc::new(upstream);
                        sessions.insert(key, upstream.clone());
                        let _ = events.send(GameEvent::PlayerOpened { peer: datagram.peer, session });
                        let replies = pump_server_replies(peer.clone(), datagram.peer, session, upstream.clone(), idle_timeout);
                        let closed_sender = closed_sender.clone();
                        tokio::spawn(async move {
                            let reason = replies.await;
                            let _ = closed_sender.send((key, reason));
                        });
                        upstream
                    }
                };
                if upstream.socket.send(payload).await.is_ok() {
                    upstream.meter.add_sent(payload.len());
                }
            }
            Some(((guest, session), reason)) = closed.recv() => {
                if let Some(upstream) = sessions.remove(&(guest, session)) {
                    let _ = events.send(GameEvent::PlayerClosed { peer: guest, session, traffic: upstream.meter.traffic(), reason });
                }
            }
            stream = streams.recv() => {
                let Some(stream) = stream else { return };
                let events = events.clone();
                tokio::spawn(async move {
                    let _ = events.send(GameEvent::DownloadOpened { peer: stream.peer });
                    let (traffic, error) = match TcpStream::connect(server).await {
                        Ok(tcp) => {
                            let (traffic, error) = splice(tcp, stream.send, stream.receive).await;
                            (Traffic { sent: traffic.received, received: traffic.sent }, error)
                        }
                        Err(error) => (Traffic::default(), Some(format!("the test server is not reachable: {error}"))),
                    };
                    let _ = events.send(GameEvent::DownloadClosed { peer: stream.peer, traffic, error });
                });
            }
        }
    }
}

struct Player {
    session: u32,
    meter: TrafficMeter,
}

pub struct GameEntrance {
    datagrams: UdpSocket,
    streams: TcpListener,
}

impl GameEntrance {
    pub async fn bind(listen: SocketAddr) -> io::Result<Self> {
        let streams = TcpListener::bind(listen).await?;
        let datagrams = UdpSocket::bind(streams.local_addr()?).await?;
        Ok(Self { datagrams, streams })
    }

    pub fn local_address(&self) -> io::Result<SocketAddr> {
        self.streams.local_addr()
    }

    pub async fn run(self, peer: PeerConnection, mut replies: UnboundedReceiver<Datagram>, idle_timeout: Duration, events: UnboundedSender<GameEvent>) {
        let listener = self.streams;
        let stream_peer = peer.clone();
        let stream_events = events.clone();
        let streams = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let peer = stream_peer.clone();
                let events = stream_events.clone();
                tokio::spawn(async move {
                    let _ = events.send(GameEvent::DownloadOpened { peer: HOST });
                    let (traffic, error) = match peer.open_stream(HOST, Channel::GameStream).await {
                        Ok((send, receive)) => splice(tcp, send, receive).await,
                        Err(error) => (Traffic::default(), Some(error.to_string())),
                    };
                    let _ = events.send(GameEvent::DownloadClosed { peer: HOST, traffic, error });
                });
            }
        });

        let socket = self.datagrams;
        let mut players: HashMap<SocketAddr, Player> = HashMap::new();
        let mut addresses: HashMap<u32, SocketAddr> = HashMap::new();
        let mut next_session = 1u32;
        let mut buffer = vec![0; MAX_DATAGRAM];
        let mut sweep = tokio::time::interval(idle_timeout.min(Duration::from_secs(1)));
        loop {
            tokio::select! {
                received = socket.recv_from(&mut buffer) => {
                    let (length, client) = match received {
                        Ok(received) => received,
                        Err(error) if is_transient(&error) => continue,
                        Err(_) => break,
                    };
                    let player = players.entry(client).or_insert_with(|| {
                        let session = next_session;
                        next_session = next_session.wrapping_add(1).max(1);
                        addresses.insert(session, client);
                        let _ = events.send(GameEvent::PlayerOpened { peer: HOST, session });
                        Player { session, meter: TrafficMeter::default() }
                    });
                    player.meter.add_sent(length);
                    peer.send_datagram(HOST, Channel::GameDatagram, &frame(player.session, &buffer[..length]));
                }
                reply = replies.recv() => {
                    let Some(reply) = reply else { break };
                    let Some((session, payload)) = unframe(&reply.payload) else { continue };
                    let Some(client) = addresses.get(&session).copied() else { continue };
                    if let Some(player) = players.get(&client) {
                        player.meter.add_received(payload.len());
                    }
                    let _ = socket.send_to(payload, client).await;
                }
                _ = sweep.tick() => {
                    players.retain(|_, player| {
                        if player.meter.idle_for() < idle_timeout {
                            return true;
                        }
                        addresses.remove(&player.session);
                        let _ = events.send(GameEvent::PlayerClosed { peer: HOST, session: player.session, traffic: player.meter.traffic(), reason: "idle".to_owned() });
                        false
                    });
                }
            }
        }
        streams.abort();
    }
}
