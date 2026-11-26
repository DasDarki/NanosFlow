use std::io;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const ALPN: &[u8] = b"nanosflow-relay/1";
pub const DEFAULT_PORT: u16 = 7900;
pub const HOST: PeerId = 0;
pub const HEADER_LENGTH: usize = 5;
pub const MAX_MESSAGE_LENGTH: usize = 64 * 1024;
pub const MAX_DATAGRAM_LENGTH: usize = 64 * 1024;

pub const CLOSE_LEFT: u32 = 0;
pub const CLOSE_REFUSED: u32 = 1;
pub const CLOSE_ROOM_CLOSED: u32 = 2;
pub const CLOSE_KICKED: u32 = 3;
pub const CLOSE_PROTOCOL: u32 = 4;

pub type PeerId = u32;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    Sync = 1,
    Presence = 2,
    GameDatagram = 3,
    GameStream = 4,
}

impl Channel {
    pub fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Sync),
            2 => Some(Self::Presence),
            3 => Some(Self::GameDatagram),
            4 => Some(Self::GameStream),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub peer: PeerId,
    pub channel: Channel,
}

impl Header {
    pub fn new(peer: PeerId, channel: Channel) -> Self {
        Self { peer, channel }
    }

    pub fn encode(self) -> [u8; HEADER_LENGTH] {
        let peer = self.peer.to_be_bytes();
        [peer[0], peer[1], peer[2], peer[3], self.channel as u8]
    }

    pub fn decode(bytes: &[u8]) -> Option<(Self, &[u8])> {
        let (header, payload) = bytes.split_at_checked(HEADER_LENGTH)?;
        let peer = PeerId::from_be_bytes([header[0], header[1], header[2], header[3]]);
        Some((Self::new(peer, Channel::from_byte(header[4])?), payload))
    }

    pub fn frame(self, payload: &[u8]) -> Vec<u8> {
        let mut framed = Vec::with_capacity(HEADER_LENGTH + payload.len());
        framed.extend_from_slice(&self.encode());
        framed.extend_from_slice(payload);
        framed
    }
}

pub async fn write_header(writer: &mut (impl AsyncWrite + Unpin), header: Header) -> io::Result<()> {
    writer.write_all(&header.encode()).await
}

pub async fn read_header(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<Header> {
    let mut bytes = [0; HEADER_LENGTH];
    reader.read_exact(&mut bytes).await?;
    Header::decode(&bytes)
        .map(|(header, _)| header)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown channel"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ClientMessage {
    CreateRoom { name: String },
    JoinRoom { code: String, name: String },
    Admit { guest: PeerId },
    Refuse { guest: PeerId, reason: String },
    Kick { guest: PeerId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ServerMessage {
    RoomCreated { code: String },
    JoinRequested { guest: PeerId, name: String },
    GuestLeft { guest: PeerId },
    Joined { id: PeerId, host: String },
}

pub async fn write_message<T: Serialize>(writer: &mut (impl AsyncWrite + Unpin), message: &T) -> io::Result<()> {
    write_message_limited(writer, message, MAX_MESSAGE_LENGTH).await
}

pub async fn read_message<T: DeserializeOwned>(reader: &mut (impl AsyncRead + Unpin)) -> io::Result<Option<T>> {
    read_message_limited(reader, MAX_MESSAGE_LENGTH).await
}

pub async fn write_message_limited<T: Serialize>(writer: &mut (impl AsyncWrite + Unpin), message: &T, limit: usize) -> io::Result<()> {
    let encoded = serde_json::to_vec(message).map_err(io::Error::other)?;
    if encoded.len() > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "message too large"));
    }
    writer.write_all(&(encoded.len() as u32).to_be_bytes()).await?;
    writer.write_all(&encoded).await
}

pub async fn read_message_limited<T: DeserializeOwned>(reader: &mut (impl AsyncRead + Unpin), limit: usize) -> io::Result<Option<T>> {
    let mut length = [0; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error),
    }
    let length = u32::from_be_bytes(length) as usize;
    if length > limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }
    let mut encoded = vec![0; length];
    reader.read_exact(&mut encoded).await?;
    serde_json::from_slice(&encoded).map(Some).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789";
pub const CODE_LENGTH: usize = 6;

pub fn generate_code() -> String {
    use rand::Rng;
    let mut random = rand::rng();
    (0..CODE_LENGTH).map(|_| CODE_ALPHABET[random.random_range(0..CODE_ALPHABET.len())] as char).collect()
}

pub fn normalize_code(code: &str) -> String {
    code.chars().filter(char::is_ascii_alphanumeric).map(|character| character.to_ascii_uppercase()).collect()
}

pub fn display_code(code: &str) -> String {
    let (first, second) = code.split_at(code.len() / 2);
    format!("{first}-{second}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_headers_and_codes() {
        let framed = Header::new(7, Channel::GameDatagram).frame(b"data");
        assert_eq!(Header::decode(&framed), Some((Header::new(7, Channel::GameDatagram), b"data".as_slice())));
        assert_eq!(Header::decode(&[0, 0, 0, 1, 99]), None);
        assert_eq!(Header::decode(&[0, 0]), None);
        let code = generate_code();
        assert_eq!(code.len(), CODE_LENGTH);
        assert_eq!(normalize_code(&display_code(&code).to_lowercase()), code);
    }

    #[tokio::test]
    async fn frames_messages() {
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        let message = ClientMessage::JoinRoom { code: "ABC123".into(), name: "Guest".into() };
        write_message(&mut writer, &message).await.unwrap();
        drop(writer);
        assert_eq!(read_message::<ClientMessage>(&mut reader).await.unwrap(), Some(message));
        assert_eq!(read_message::<ClientMessage>(&mut reader).await.unwrap(), None);
    }
}
