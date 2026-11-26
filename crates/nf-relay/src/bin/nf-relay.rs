use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use nf_relay::protocol::DEFAULT_PORT;
use nf_relay::{Identity, RelayServer};
use tokio::sync::mpsc;

const USAGE: &str = "Usage: nf-relay [--listen <address>] [--identity <directory>] [--name <public host name>] [--certificate <cert.pem> --key <key.pem>]

  --listen       UDP address to listen on (default 0.0.0.0:7900)
  --identity     folder for the generated self-signed certificate (default ./relay-identity)
  --name         host name written into a newly generated certificate (default localhost)
  --certificate  PEM certificate chain, for example from Let's Encrypt; clients then need no fingerprint
  --key          PEM private key that belongs to --certificate";

struct Options {
    listen: SocketAddr,
    identity: PathBuf,
    names: Vec<String>,
    certificate: Option<(PathBuf, PathBuf)>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        listen: SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)),
        identity: PathBuf::from("relay-identity"),
        names: Vec::new(),
        certificate: None,
    };
    let mut certificate = None;
    let mut key = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(flag) = arguments.next() {
        let mut value = || arguments.next().ok_or_else(|| format!("{flag} needs a value.\n\n{USAGE}"));
        match flag.as_str() {
            "--listen" => options.listen = value()?.parse().map_err(|_| format!("--listen expects an address like 0.0.0.0:7900.\n\n{USAGE}"))?,
            "--identity" => options.identity = PathBuf::from(value()?),
            "--name" => options.names.push(value()?),
            "--certificate" => certificate = Some(PathBuf::from(value()?)),
            "--key" => key = Some(PathBuf::from(value()?)),
            "--help" | "-h" => return Err(USAGE.to_owned()),
            other => return Err(format!("Unknown option {other}.\n\n{USAGE}")),
        }
    }
    match (certificate, key) {
        (Some(certificate), Some(key)) => options.certificate = Some((certificate, key)),
        (None, None) => {}
        _ => return Err(format!("--certificate and --key must be given together.\n\n{USAGE}")),
    }
    if options.names.is_empty() {
        options.names.push("localhost".to_owned());
    }
    Ok(options)
}

#[tokio::main]
async fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let identity = match &options.certificate {
        Some((certificate, key)) => Identity::from_pem_files(certificate, key),
        None => Identity::load_or_generate(&options.identity, options.names.clone()),
    };
    let identity = match identity {
        Ok(identity) => identity,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let server = match RelayServer::bind(options.listen, &identity) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let port = server.local_address().port();
    println!("NanosFlow relay listening on {} (UDP).", server.local_address());
    if options.certificate.is_some() {
        println!("Clients connect with: <host>:{port}");
    } else {
        println!("Clients connect with: <host>:{port}#{}", identity.fingerprint());
    }
    let (events, mut received) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = received.recv().await {
            println!("{event}");
        }
    });
    tokio::select! {
        _ = server.run(events) => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    ExitCode::SUCCESS
}
