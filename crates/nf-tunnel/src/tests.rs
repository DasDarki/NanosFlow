use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::time::timeout;

use crate::{PortForward, Traffic, TunnelEvent};

const WAIT: Duration = Duration::from_secs(5);

async fn echo_server() -> (SocketAddr, UnboundedReceiver<SocketAddr>) {
    let datagrams = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = datagrams.local_addr().unwrap();
    let streams = TcpListener::bind(address).await.unwrap();
    let (senders, seen) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut buffer = [0; 2048];
        loop {
            let (length, sender) = datagrams.recv_from(&mut buffer).await.unwrap();
            let _ = senders.send(sender);
            datagrams.send_to(&buffer[..length], sender).await.unwrap();
        }
    });
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = streams.accept().await.unwrap();
            tokio::spawn(async move {
                let (mut reader, mut writer) = stream.split();
                let _ = tokio::io::copy(&mut reader, &mut writer).await;
            });
        }
    });
    (address, seen)
}

async fn start_forward(target: SocketAddr, idle_timeout: Duration) -> (SocketAddr, UnboundedReceiver<TunnelEvent>) {
    let forward = PortForward::bind("127.0.0.1:0".parse().unwrap(), target).await.unwrap().with_idle_timeout(idle_timeout);
    let address = forward.local_address().unwrap();
    let (events, received) = mpsc::unbounded_channel();
    tokio::spawn(forward.run(events));
    (address, received)
}

async fn next_event(events: &mut UnboundedReceiver<TunnelEvent>) -> TunnelEvent {
    timeout(WAIT, events.recv()).await.expect("no tunnel event").unwrap()
}

#[tokio::test]
async fn gives_every_player_its_own_server_side_socket() {
    let (server, mut seen) = echo_server().await;
    let (forward, mut events) = start_forward(server, Duration::from_secs(60)).await;
    let mut upstreams = HashSet::new();
    for message in [b"first".as_slice(), b"second"] {
        let player = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        player.send_to(message, forward).await.unwrap();
        player.send_to(message, forward).await.unwrap();
        for _ in 0..2 {
            let mut buffer = [0; 64];
            let (length, from) = timeout(WAIT, player.recv_from(&mut buffer)).await.unwrap().unwrap();
            assert_eq!((&buffer[..length], from), (message, forward));
        }
        let TunnelEvent::DatagramSessionOpened { client, upstream } = next_event(&mut events).await else {
            panic!("expected a new session");
        };
        assert_eq!(client, player.local_addr().unwrap());
        upstreams.insert(upstream.port());
        for _ in 0..2 {
            assert_eq!(seen.recv().await.unwrap().port(), upstream.port());
        }
    }
    assert_eq!(upstreams.len(), 2);
}

#[tokio::test]
async fn closes_idle_sessions_with_their_traffic() {
    let (server, _seen) = echo_server().await;
    let (forward, mut events) = start_forward(server, Duration::from_millis(200)).await;
    let player = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    player.send_to(b"ping", forward).await.unwrap();
    assert!(matches!(next_event(&mut events).await, TunnelEvent::DatagramSessionOpened { .. }));
    let closed = next_event(&mut events).await;
    assert_eq!(
        closed,
        TunnelEvent::DatagramSessionClosed {
            client: player.local_addr().unwrap(),
            traffic: Traffic { sent: 4, received: 4 },
            reason: "idle".to_owned(),
        }
    );
}

#[tokio::test]
async fn forwards_streams_on_the_same_port() {
    let (server, _seen) = echo_server().await;
    let (forward, mut events) = start_forward(server, Duration::from_secs(60)).await;
    let mut stream = TcpStream::connect(forward).await.unwrap();
    let payload = vec![7u8; 300_000];
    stream.write_all(&payload).await.unwrap();
    let mut echoed = vec![0; payload.len()];
    timeout(WAIT, stream.read_exact(&mut echoed)).await.unwrap().unwrap();
    assert_eq!(echoed, payload);
    drop(stream);
    assert!(matches!(next_event(&mut events).await, TunnelEvent::StreamOpened { .. }));
    let TunnelEvent::StreamClosed { traffic, error, .. } = next_event(&mut events).await else {
        panic!("expected the stream to close");
    };
    assert_eq!((traffic, error), (Traffic { sent: 300_000, received: 300_000 }, None));
}

#[tokio::test]
async fn reports_a_server_that_is_not_running() {
    let unused = UdpSocket::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();
    let (forward, mut events) = start_forward(unused, Duration::from_secs(60)).await;
    let player = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    player.send_to(b"hello", forward).await.unwrap();
    assert!(matches!(next_event(&mut events).await, TunnelEvent::DatagramSessionOpened { .. }));
    let TunnelEvent::DatagramSessionClosed { reason, .. } = next_event(&mut events).await else {
        panic!("expected the session to close");
    };
    assert_eq!(reason, "the server is not listening");
}
