use std::net::SocketAddr;
use std::time::Duration;

use nf_relay::{Identity, RelayServer};
use nf_session::{GameEntrance, GameEvent, HostEvent, RelayAddress, SessionError, host, join, serve_game};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(10);

async fn start_relay() -> RelayAddress {
    let identity = Identity::generate(vec!["localhost".into()]).unwrap();
    let server = RelayServer::bind("127.0.0.1:0".parse().unwrap(), &identity).unwrap();
    let address = RelayAddress {
        host: "127.0.0.1".into(),
        port: server.local_address().port(),
        fingerprint: Some(identity.fingerprint()),
    };
    let (events, _) = mpsc::unbounded_channel();
    tokio::spawn(server.run(events));
    address
}

async fn echo_server() -> SocketAddr {
    let datagrams = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = datagrams.local_addr().unwrap();
    let streams = TcpListener::bind(address).await.unwrap();
    tokio::spawn(async move {
        let mut buffer = vec![0; 65_535];
        loop {
            let (length, sender) = datagrams.recv_from(&mut buffer).await.unwrap();
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
    address
}

async fn next_host_event(events: &mut UnboundedReceiver<HostEvent>) -> HostEvent {
    timeout(WAIT, events.recv()).await.expect("no host event").unwrap()
}

async fn next_game_event(events: &mut UnboundedReceiver<GameEvent>) -> GameEvent {
    timeout(WAIT, events.recv()).await.expect("no game event").unwrap()
}

#[tokio::test]
async fn tunnels_the_game_server_to_an_admitted_guest() {
    let relay = start_relay().await;
    let server = echo_server().await;
    let mut hosted = host(&relay, "Host").await.unwrap();
    assert_eq!(hosted.code.len(), 6);

    let code = nf_session::display_code(&hosted.code).to_lowercase();
    let joining = tokio::spawn({
        let relay = relay.clone();
        async move { join(&relay, &code, "Guest").await }
    });
    let HostEvent::JoinRequested { guest, name } = next_host_event(&mut hosted.events).await else {
        panic!("expected a join request");
    };
    assert_eq!(name, "Guest");
    hosted.control.admit(guest);
    let joined = timeout(WAIT, joining).await.unwrap().unwrap().unwrap();
    assert_eq!((joined.id, joined.host_name.as_str()), (guest, "Host"));

    let (host_events, mut host_game) = mpsc::unbounded_channel();
    tokio::spawn(serve_game(hosted.peer.clone(), hosted.inbox.game_datagrams, hosted.inbox.game_streams, server, Duration::from_secs(60), host_events));
    let entrance = GameEntrance::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let entrance_address = entrance.local_address().unwrap();
    let (guest_events, _guest_game) = mpsc::unbounded_channel();
    tokio::spawn(entrance.run(joined.peer.clone(), joined.inbox.game_datagrams, Duration::from_secs(60), guest_events));

    let player = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    for payload in [vec![1u8; 32], vec![2u8; 5000]] {
        player.send_to(&payload, entrance_address).await.unwrap();
        let mut buffer = vec![0; 65_535];
        let (length, from) = timeout(WAIT, player.recv_from(&mut buffer)).await.expect("no reply through the relay").unwrap();
        assert_eq!((&buffer[..length], from), (payload.as_slice(), entrance_address));
    }
    assert_eq!(next_game_event(&mut host_game).await, GameEvent::PlayerOpened { peer: guest, session: 1 });

    let mut download = TcpStream::connect(entrance_address).await.unwrap();
    let payload = vec![9u8; 400_000];
    download.write_all(&payload).await.unwrap();
    let mut echoed = vec![0; payload.len()];
    timeout(WAIT, download.read_exact(&mut echoed)).await.unwrap().unwrap();
    assert_eq!(echoed, payload);
    drop(download);
    assert_eq!(next_game_event(&mut host_game).await, GameEvent::DownloadOpened { peer: guest });

    joined.peer.close();
    loop {
        if let HostEvent::GuestLeft { guest: left } = next_host_event(&mut hosted.events).await {
            assert_eq!(left, guest);
            break;
        }
    }
}

#[tokio::test]
async fn refuses_guests_and_unknown_codes() {
    let relay = start_relay().await;
    let mut hosted = host(&relay, "Host").await.unwrap();
    let joining = tokio::spawn({
        let relay = relay.clone();
        let code = hosted.code.clone();
        async move { join(&relay, &code, "Guest").await }
    });
    let HostEvent::JoinRequested { guest, .. } = next_host_event(&mut hosted.events).await else {
        panic!("expected a join request");
    };
    hosted.control.refuse(guest, "Not today.");
    let refused = timeout(WAIT, joining).await.unwrap().unwrap();
    assert!(matches!(refused, Err(SessionError::Refused(reason)) if reason == "Not today."));

    let unknown = join(&relay, "ZZZZZZ", "Guest").await;
    assert!(matches!(unknown, Err(SessionError::Refused(reason)) if reason == "There is no session with this code."));
}

#[tokio::test]
async fn ends_the_session_for_guests_when_the_host_leaves() {
    let relay = start_relay().await;
    let mut hosted = host(&relay, "Host").await.unwrap();
    let joining = tokio::spawn({
        let relay = relay.clone();
        let code = hosted.code.clone();
        async move { join(&relay, &code, "Guest").await }
    });
    let HostEvent::JoinRequested { guest, .. } = next_host_event(&mut hosted.events).await else {
        panic!("expected a join request");
    };
    hosted.control.admit(guest);
    let joined = timeout(WAIT, joining).await.unwrap().unwrap().unwrap();
    hosted.peer.close();
    let reason = timeout(WAIT, joined.peer.closed()).await.unwrap();
    assert_eq!(reason, "The host ended the session.");
}

#[tokio::test]
async fn rejects_a_relay_with_another_certificate() {
    let mut relay = start_relay().await;
    relay.fingerprint = Some("00".repeat(32));
    assert!(matches!(host(&relay, "Host").await, Err(SessionError::Connect(_))));
}

async fn eventually(description: &str, mut check: impl FnMut() -> bool) {
    let started = std::time::Instant::now();
    while !check() {
        assert!(started.elapsed() < WAIT, "timed out waiting for {description}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn read(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

async fn admitted_guest(relay: &RelayAddress, code: &str, events: &mut UnboundedReceiver<HostEvent>, control: &nf_session::HostControl, name: &str) -> nf_session::JoinedSession {
    let joining = tokio::spawn({
        let relay = relay.clone();
        let code = code.to_owned();
        let name = name.to_owned();
        async move { join(&relay, &code, &name).await }
    });
    let HostEvent::JoinRequested { guest, .. } = next_host_event(events).await else {
        panic!("expected a join request");
    };
    control.admit(guest);
    timeout(WAIT, joining).await.unwrap().unwrap().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn syncs_project_files_between_host_and_guests() {
    let relay = start_relay().await;
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    std::fs::write(root.join("Game.nfproj"), "name = \"Game\"\n").unwrap();
    let graph = root.join("packages/game/graphs/server/Main");
    std::fs::create_dir_all(graph.join("nodes")).unwrap();
    std::fs::write(graph.join("graph.toml"), "kind = \"event-graph\"\n").unwrap();
    std::fs::write(graph.join("nodes/a.toml"), "position = [0.0, 0.0]\n").unwrap();
    std::fs::create_dir_all(root.join(".nanosflow")).unwrap();
    std::fs::write(root.join(".nanosflow/settings.toml"), "private = true\n").unwrap();

    let mut hosted = host(&relay, "Host").await.unwrap();
    let (host_events, mut host_sync_events) = mpsc::unbounded_channel();
    let _host_sync = nf_session::host_sync(root.to_path_buf(), "Game.nfproj".into(), hosted.inbox.sync_streams, host_events).unwrap();

    let first = admitted_guest(&relay, &hosted.code, &mut hosted.events, &hosted.control, "First").await;
    let second = admitted_guest(&relay, &hosted.code, &mut hosted.events, &hosted.control, "Second").await;
    let first_folder = tempfile::tempdir().unwrap();
    let second_folder = tempfile::tempdir().unwrap();
    let (first_events, _first_received) = mpsc::unbounded_channel();
    let (second_events, mut second_received) = mpsc::unbounded_channel();
    let (_first_sync, first_project) = nf_session::guest_sync(first_folder.path().to_path_buf(), &first.peer, first_events).await.unwrap();
    _host_sync.set_game(nf_session::GameState { running: true, steam_app: "playtest".into() });
    let (_second_sync, _) = nf_session::guest_sync(second_folder.path().to_path_buf(), &second.peer, second_events).await.unwrap();

    assert_eq!(first_project, first_folder.path().join("Game.nfproj"));
    let first_graph = first_folder.path().join("packages/game/graphs/server/Main");
    let second_graph = second_folder.path().join("packages/game/graphs/server/Main");
    assert_eq!(read(&first_graph.join("nodes/a.toml")).as_deref(), Some("position = [0.0, 0.0]\n"));
    assert!(!first_folder.path().join(".nanosflow").exists());

    std::fs::write(first_graph.join("nodes/b.toml"), "position = [5.0, 5.0]\n").unwrap();
    eventually("the guest node on the host", || read(&graph.join("nodes/b.toml")).is_some()).await;
    eventually("the guest node on the other guest", || read(&second_graph.join("nodes/b.toml")).is_some()).await;
    loop {
        match timeout(WAIT, second_received.recv()).await.unwrap().unwrap() {
            nf_session::SyncEvent::Game { state } if state.running => {
                assert_eq!(state.steam_app, "playtest");
                break;
            }
            _ => {}
        }
    }
    let applied = timeout(WAIT, host_sync_events.recv()).await.unwrap().unwrap();
    assert!(matches!(applied, nf_session::SyncEvent::Applied { ref paths, .. } if paths == &["packages/game/graphs/server/Main/nodes/b.toml"]));
    loop {
        if let nf_session::SyncEvent::Applied { paths, author } = timeout(WAIT, second_received.recv()).await.unwrap().unwrap() {
            assert_eq!((paths.len(), author), (1, nf_session::PeerId::default()));
            break;
        }
    }

    std::fs::write(graph.join("nodes/a.toml"), "position = [9.0, 9.0]\n").unwrap();
    std::fs::remove_file(graph.join("nodes/b.toml")).unwrap();
    eventually("the host edit on the first guest", || read(&first_graph.join("nodes/a.toml")).as_deref() == Some("position = [9.0, 9.0]\n")).await;
    eventually("the host deletion on both guests", || !first_graph.join("nodes/b.toml").exists() && !second_graph.join("nodes/b.toml").exists()).await;

    for round in 0..5 {
        std::fs::write(first_graph.join("nodes/a.toml"), format!("position = [{round}.0, 1.0]\n")).unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    let settled = |path: &std::path::Path| read(path).as_deref() == Some("position = [4.0, 1.0]\n");
    eventually("rapid guest edits converging everywhere", || settled(&graph.join("nodes/a.toml")) && settled(&first_graph.join("nodes/a.toml")) && settled(&second_graph.join("nodes/a.toml"))).await;
}
