use std::io;
use std::net::SocketAddr;

use tokio::io::copy_bidirectional;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;

use crate::{Traffic, TunnelEvent};

async fn forward(mut inbound: TcpStream, target: SocketAddr, traffic: &mut Traffic) -> io::Result<()> {
    let mut outbound = TcpStream::connect(target).await?;
    inbound.set_nodelay(true)?;
    outbound.set_nodelay(true)?;
    let (sent, received) = copy_bidirectional(&mut inbound, &mut outbound).await?;
    *traffic = Traffic { sent, received };
    Ok(())
}

pub(crate) async fn relay(listener: TcpListener, target: SocketAddr, events: UnboundedSender<TunnelEvent>) -> io::Result<()> {
    loop {
        let (inbound, client) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) if matches!(error.kind(), io::ErrorKind::ConnectionAborted | io::ErrorKind::ConnectionReset | io::ErrorKind::Interrupted) => continue,
            Err(error) => return Err(error),
        };
        let events = events.clone();
        tokio::spawn(async move {
            let _ = events.send(TunnelEvent::StreamOpened { client });
            let mut traffic = Traffic::default();
            let error = forward(inbound, target, &mut traffic).await.err().map(|error| error.to_string());
            let _ = events.send(TunnelEvent::StreamClosed { client, traffic, error });
        });
    }
}
