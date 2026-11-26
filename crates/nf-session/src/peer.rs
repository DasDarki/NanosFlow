use bytes::Bytes;
use nf_relay::protocol::{CLOSE_LEFT, Channel, Header, MAX_DATAGRAM_LENGTH, PeerId, read_header, write_header};
use quinn::{Connection, ConnectionError, Endpoint, RecvStream, SendStream, VarInt};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::SessionError;

#[derive(Clone)]
pub struct PeerConnection {
    connection: Connection,
    _endpoint: Endpoint,
}

pub fn describe_close(error: &ConnectionError) -> String {
    match error {
        ConnectionError::ApplicationClosed(close) => String::from_utf8_lossy(&close.reason).into_owned(),
        ConnectionError::TimedOut => "The connection to the relay timed out.".to_owned(),
        ConnectionError::LocallyClosed => "You left the session.".to_owned(),
        other => other.to_string(),
    }
}

impl PeerConnection {
    pub(crate) fn new(endpoint: Endpoint, connection: Connection) -> Self {
        Self { connection, _endpoint: endpoint }
    }

    pub fn send_datagram(&self, to: PeerId, channel: Channel, payload: &[u8]) {
        nf_relay::deliver_datagram(&self.connection, Header::new(to, channel), payload);
    }

    pub async fn open_stream(&self, to: PeerId, channel: Channel) -> Result<(SendStream, RecvStream), SessionError> {
        let (mut send, receive) = self.connection.open_bi().await.map_err(|error| SessionError::Ended(describe_close(&error)))?;
        write_header(&mut send, Header::new(to, channel)).await?;
        Ok((send, receive))
    }

    pub async fn closed(&self) -> String {
        describe_close(&self.connection.closed().await)
    }

    pub fn close(&self) {
        self.connection.close(VarInt::from_u32(CLOSE_LEFT), b"left");
    }
}

#[derive(Debug)]
pub struct Datagram {
    pub peer: PeerId,
    pub channel: Channel,
    pub payload: Bytes,
}

pub struct IncomingStream {
    pub peer: PeerId,
    pub channel: Channel,
    pub send: SendStream,
    pub receive: RecvStream,
}

pub struct Inbox {
    pub game_datagrams: UnboundedReceiver<Datagram>,
    pub game_streams: UnboundedReceiver<IncomingStream>,
    pub sync_streams: UnboundedReceiver<IncomingStream>,
    pub presence: UnboundedReceiver<Datagram>,
}

#[derive(Clone)]
struct Routes {
    game_datagrams: UnboundedSender<Datagram>,
    game_streams: UnboundedSender<IncomingStream>,
    sync_streams: UnboundedSender<IncomingStream>,
    presence: UnboundedSender<Datagram>,
}

impl Routes {
    fn datagram(&self, header: Header, payload: Bytes) {
        let route = match header.channel {
            Channel::GameDatagram => &self.game_datagrams,
            Channel::Presence => &self.presence,
            Channel::Sync | Channel::GameStream => return,
        };
        let _ = route.send(Datagram {
            peer: header.peer,
            channel: header.channel,
            payload,
        });
    }

    fn stream(&self, header: Header, send: SendStream, receive: RecvStream) {
        let route = match header.channel {
            Channel::GameStream => &self.game_streams,
            Channel::Sync => &self.sync_streams,
            Channel::GameDatagram | Channel::Presence => return,
        };
        let _ = route.send(IncomingStream {
            peer: header.peer,
            channel: header.channel,
            send,
            receive,
        });
    }
}

pub(crate) fn start_inbox(connection: &Connection) -> Inbox {
    let (game_datagrams, game_datagrams_received) = mpsc::unbounded_channel();
    let (game_streams, game_streams_received) = mpsc::unbounded_channel();
    let (sync_streams, sync_streams_received) = mpsc::unbounded_channel();
    let (presence, presence_received) = mpsc::unbounded_channel();
    let routes = Routes {
        game_datagrams,
        game_streams,
        sync_streams,
        presence,
    };

    let datagram_connection = connection.clone();
    let datagram_routes = routes.clone();
    tokio::spawn(async move {
        while let Ok(datagram) = datagram_connection.read_datagram().await {
            if let Some((header, _)) = Header::decode(&datagram) {
                datagram_routes.datagram(header, datagram.slice(nf_relay::protocol::HEADER_LENGTH..));
            }
        }
    });

    let unidirectional_connection = connection.clone();
    let unidirectional_routes = routes.clone();
    tokio::spawn(async move {
        while let Ok(mut receive) = unidirectional_connection.accept_uni().await {
            let routes = unidirectional_routes.clone();
            tokio::spawn(async move {
                let Ok(header) = read_header(&mut receive).await else {
                    return;
                };
                if let Ok(payload) = receive.read_to_end(MAX_DATAGRAM_LENGTH).await {
                    routes.datagram(header, payload.into());
                }
            });
        }
    });

    let stream_connection = connection.clone();
    tokio::spawn(async move {
        while let Ok((send, mut receive)) = stream_connection.accept_bi().await {
            let routes = routes.clone();
            tokio::spawn(async move {
                if let Ok(header) = read_header(&mut receive).await {
                    routes.stream(header, send, receive);
                }
            });
        }
    });

    Inbox {
        game_datagrams: game_datagrams_received,
        game_streams: game_streams_received,
        sync_streams: sync_streams_received,
        presence: presence_received,
    }
}
