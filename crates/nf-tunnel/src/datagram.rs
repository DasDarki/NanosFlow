use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::time::timeout;

use crate::{TrafficMeter, TunnelEvent};

const MAX_DATAGRAM: usize = 65_535;

struct Session {
    upstream: Arc<UdpSocket>,
    meter: TrafficMeter,
}

pub fn is_transient(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused | io::ErrorKind::Interrupted)
}

async fn open_session(target: SocketAddr) -> io::Result<Session> {
    let unspecified: SocketAddr = if target.is_ipv4() { (Ipv4Addr::UNSPECIFIED, 0).into() } else { (Ipv6Addr::UNSPECIFIED, 0).into() };
    let upstream = UdpSocket::bind(unspecified).await?;
    upstream.connect(target).await?;
    Ok(Session {
        upstream: Arc::new(upstream),
        meter: TrafficMeter::default(),
    })
}

async fn pump_replies(socket: Arc<UdpSocket>, client: SocketAddr, session: Arc<Session>, idle_timeout: Duration) -> String {
    let mut buffer = vec![0; MAX_DATAGRAM];
    let check_interval = idle_timeout.min(Duration::from_secs(1));
    loop {
        match timeout(check_interval, session.upstream.recv(&mut buffer)).await {
            Ok(Ok(length)) => {
                session.meter.add_received(length);
                if let Err(error) = socket.send_to(&buffer[..length], client).await {
                    return format!("could not reply to the player: {error}");
                }
            }
            Ok(Err(error)) if error.kind() == io::ErrorKind::ConnectionRefused => return "the server is not listening".to_owned(),
            Ok(Err(error)) if is_transient(&error) => {}
            Ok(Err(error)) => return format!("server socket failed: {error}"),
            Err(_) if session.meter.idle_for() >= idle_timeout => return "idle".to_owned(),
            Err(_) => {}
        }
    }
}

pub(crate) async fn relay(socket: Arc<UdpSocket>, target: SocketAddr, idle_timeout: Duration, events: UnboundedSender<TunnelEvent>) -> io::Result<()> {
    let mut sessions: HashMap<SocketAddr, Arc<Session>> = HashMap::new();
    let (closed_sender, mut closed) = mpsc::unbounded_channel::<(SocketAddr, String)>();
    let mut buffer = vec![0; MAX_DATAGRAM];
    loop {
        tokio::select! {
            received = socket.recv_from(&mut buffer) => {
                let (length, client) = match received {
                    Ok(received) => received,
                    Err(error) if is_transient(&error) => continue,
                    Err(error) => return Err(error),
                };
                let session = match sessions.get(&client) {
                    Some(session) => session.clone(),
                    None => {
                        let session = Arc::new(open_session(target).await?);
                        let upstream = session.upstream.local_addr()?;
                        sessions.insert(client, session.clone());
                        let _ = events.send(TunnelEvent::DatagramSessionOpened { client, upstream });
                        let replies = pump_replies(socket.clone(), client, session.clone(), idle_timeout);
                        let closed_sender = closed_sender.clone();
                        tokio::spawn(async move {
                            let reason = replies.await;
                            let _ = closed_sender.send((client, reason));
                        });
                        session
                    }
                };
                if session.upstream.send(&buffer[..length]).await.is_ok() {
                    session.meter.add_sent(length);
                }
            }
            Some((client, reason)) = closed.recv() => {
                if let Some(session) = sessions.remove(&client) {
                    let _ = events.send(TunnelEvent::DatagramSessionClosed { client, traffic: session.meter.traffic(), reason });
                }
            }
        }
    }
}
