use std::collections::VecDeque;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

use nf_session::{GameEntrance, HostEvent, PeerId, RelayAddress, display_code, host, join, serve_game};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const USAGE: &str = "Usage:
  nf-session host <relay> <server address> [--name <name>] [--admit-all]
  nf-session join <relay> <code> <listen address> [--name <name>]

Examples:
  nf-session host relay.example.com:7900#<fingerprint> 127.0.0.1:7777
  nf-session join relay.example.com:7900#<fingerprint> K7Q-M2X 127.0.0.1:7787";

enum Command {
    Host { relay: RelayAddress, server: SocketAddr, name: String, admit_all: bool },
    Join { relay: RelayAddress, code: String, listen: SocketAddr, name: String },
}

fn parse_command() -> Result<Command, String> {
    let mut positional = Vec::new();
    let mut name = None;
    let mut admit_all = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--name" => name = Some(arguments.next().ok_or_else(|| format!("--name needs a value.\n\n{USAGE}"))?),
            "--admit-all" => admit_all = true,
            "--help" | "-h" => return Err(USAGE.to_owned()),
            _ => positional.push(argument),
        }
    }
    let address = |value: &str| value.parse::<SocketAddr>().map_err(|_| format!("'{value}' is not an address like 127.0.0.1:7777.\n\n{USAGE}"));
    let relay = |value: &str| value.parse::<RelayAddress>().map_err(|error| format!("{error}\n\n{USAGE}"));
    match positional.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["host", relay_value, server] => Ok(Command::Host {
            relay: relay(relay_value)?,
            server: address(server)?,
            name: name.unwrap_or_else(|| "Host".to_owned()),
            admit_all,
        }),
        ["join", relay_value, code, listen] => Ok(Command::Join {
            relay: relay(relay_value)?,
            code: (*code).to_owned(),
            listen: address(listen)?,
            name: name.unwrap_or_else(|| "Guest".to_owned()),
        }),
        _ => Err(USAGE.to_owned()),
    }
}

fn print_game_events() -> mpsc::UnboundedSender<nf_session::GameEvent> {
    let (events, mut received) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(event) = received.recv().await {
            println!("{event}");
        }
    });
    events
}

async fn run_host(relay: RelayAddress, server: SocketAddr, name: String, admit_all: bool) -> Result<(), String> {
    let mut session = host(&relay, &name).await.map_err(|error| error.to_string())?;
    println!("Session code: {}", display_code(&session.code));
    println!("Forwarding game traffic of admitted guests to {server}. Press Ctrl+C to end the session.");
    tokio::spawn(serve_game(session.peer.clone(), session.inbox.game_datagrams, session.inbox.game_streams, server, IDLE_TIMEOUT, print_game_events()));

    let mut answers = BufReader::new(tokio::io::stdin()).lines();
    let mut waiting: VecDeque<(PeerId, String)> = VecDeque::new();
    let prompt = |waiting: &VecDeque<(PeerId, String)>| {
        if let Some((_, name)) = waiting.front() {
            println!("{name} wants to join. Admit? [y/N]");
        }
    };
    loop {
        tokio::select! {
            event = session.events.recv() => match event {
                Some(HostEvent::JoinRequested { guest, name }) if admit_all => {
                    session.control.admit(guest);
                    println!("{name} joined as guest {guest}.");
                }
                Some(HostEvent::JoinRequested { guest, name }) => {
                    waiting.push_back((guest, name));
                    if waiting.len() == 1 {
                        prompt(&waiting);
                    }
                }
                Some(HostEvent::GuestLeft { guest }) => {
                    let was_first = waiting.front().is_some_and(|(waiting_guest, _)| *waiting_guest == guest);
                    waiting.retain(|(waiting_guest, _)| *waiting_guest != guest);
                    println!("Guest {guest} left.");
                    if was_first {
                        prompt(&waiting);
                    }
                }
                Some(HostEvent::Ended { reason }) => return Err(reason),
                None => return Ok(()),
            },
            answer = answers.next_line(), if !waiting.is_empty() => {
                let Ok(Some(answer)) = answer else { continue };
                if let Some((guest, name)) = waiting.pop_front() {
                    if answer.trim().eq_ignore_ascii_case("y") {
                        session.control.admit(guest);
                        println!("{name} joined as guest {guest}.");
                    } else {
                        session.control.refuse(guest, "The host declined your request.");
                        println!("{name} was declined.");
                    }
                }
                prompt(&waiting);
            }
            _ = tokio::signal::ctrl_c() => {
                session.peer.close();
                return Ok(());
            }
        }
    }
}

async fn run_join(relay: RelayAddress, code: String, listen: SocketAddr, name: String) -> Result<(), String> {
    let entrance = GameEntrance::bind(listen).await.map_err(|error| format!("Could not listen on {listen} (UDP and TCP): {error}"))?;
    let entrance_address = entrance.local_address().map_err(|error| error.to_string())?;
    println!("Waiting for the host to admit you...");
    let session = join(&relay, &code, &name).await.map_err(|error| error.to_string())?;
    println!("Joined the session of {}. Connect nanos world with Direct Connect to {entrance_address}.", session.host_name);
    tokio::spawn(entrance.run(session.peer.clone(), session.inbox.game_datagrams, IDLE_TIMEOUT, print_game_events()));
    tokio::select! {
        reason = session.peer.closed() => Err(reason),
        _ = tokio::signal::ctrl_c() => {
            session.peer.close();
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let command = match parse_command() {
        Ok(command) => command,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let result = match command {
        Command::Host { relay, server, name, admit_all } => run_host(relay, server, name, admit_all).await,
        Command::Join { relay, code, listen, name } => run_join(relay, code, listen, name).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
