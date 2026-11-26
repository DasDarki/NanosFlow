use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use nf_tunnel::PortForward;
use tokio::sync::mpsc;

const USAGE: &str = "Usage: nf-tunnel <listen address> <server address>\nExample: nf-tunnel 127.0.0.1:7787 127.0.0.1:7777\nThen connect the game with Direct Connect to the listen address.";

fn timestamp() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or_default();
    format!("{:02}:{:02}:{:02}", seconds / 3600 % 24, seconds / 60 % 60, seconds % 60)
}

fn parse_arguments() -> Result<(SocketAddr, SocketAddr), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let [listen, target] = arguments.as_slice() else {
        return Err(USAGE.to_owned());
    };
    let parse = |value: &str| value.parse::<SocketAddr>().map_err(|_| format!("'{value}' is not an address like 127.0.0.1:7777.\n\n{USAGE}"));
    Ok((parse(listen)?, parse(target)?))
}

#[tokio::main]
async fn main() -> ExitCode {
    let (listen, target) = match parse_arguments() {
        Ok(addresses) => addresses,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let forward = match PortForward::bind(listen, target).await {
        Ok(forward) => forward,
        Err(error) => {
            eprintln!("Could not listen on {listen} (UDP and TCP): {error}");
            return ExitCode::FAILURE;
        }
    };
    println!("Forwarding UDP and TCP from {listen} to {target}. Press Ctrl+C to stop.");
    let (events, mut received) = mpsc::unbounded_channel();
    let printer = tokio::spawn(async move {
        while let Some(event) = received.recv().await {
            println!("[{} UTC] {event}", timestamp());
        }
    });
    let result = tokio::select! {
        result = forward.run(events) => result,
        _ = tokio::signal::ctrl_c() => Ok(()),
    };
    printer.abort();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Forwarding stopped: {error}");
            ExitCode::FAILURE
        }
    }
}
