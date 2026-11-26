use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nf_project::Project;
use nf_relay::protocol::{Channel, HOST};
use nf_project::SteamApp;
use nf_session::{GameEntrance, GameEvent, GameState, HostControl, HostEvent, PeerConnection, PeerId, RelayAddress, SyncEvent, SyncHandle, display_code};
use serde::{Deserialize, Serialize};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::mpsc;

use crate::error::{CommandError, CommandResult};

pub const SESSION_EVENT: &str = "session";
pub const SESSION_FILES_EVENT: &str = "session-files";
pub const SESSION_PRESENCE_EVENT: &str = "session-presence";
const SESSIONS_DIRECTORY: &str = "sessions";
const MAX_SELECTION: usize = 64;
const SETTINGS_FILE: &str = "session.toml";
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const ACTIVITY_LIMIT: usize = 200;
const DEFAULT_GUEST_PORT: u16 = 7787;
const MAX_NAME_LENGTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionSettings {
    relay: String,
    name: String,
    guest_port: u16,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            relay: String::new(),
            name: String::new(),
            guest_port: DEFAULT_GUEST_PORT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    id: PeerId,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum SessionView {
    Idle,
    Starting,
    Hosting { code: String, server: String, project_file: String, game_running: bool, guests: Vec<Person>, requests: Vec<Person> },
    Waiting { code: String },
    Joined { host_name: String, address: String, project_file: String, game: GameState },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    Info,
    Success,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    id: u64,
    text: String,
    tone: Tone,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    settings: SessionSettings,
    view: SessionView,
    activity: Vec<Activity>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    title: String,
    message: String,
    tone: Tone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceUpdate {
    graph: Option<String>,
    x: f64,
    y: f64,
    selection: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    peer: PeerId,
    name: String,
    graph: Option<String>,
    x: f64,
    y: f64,
    selection: Vec<String>,
    gone: bool,
}

impl Presence {
    fn gone(peer: PeerId) -> Self {
        Self {
            peer,
            name: String::new(),
            graph: None,
            x: 0.0,
            y: 0.0,
            selection: Vec::new(),
            gone: true,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilesMessage {
    project_root: String,
    paths: Vec<String>,
    author: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionMessage {
    status: SessionStatus,
    notice: Option<Notice>,
}

struct Inner {
    generation: u64,
    view: SessionView,
    activity: VecDeque<Activity>,
    next_activity: u64,
    peer: Option<PeerConnection>,
    control: Option<HostControl>,
    sync: Option<SyncHandle>,
    own_name: String,
    server: Option<SteamApp>,
    tasks: Vec<JoinHandle<()>>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            generation: 0,
            view: SessionView::Idle,
            activity: VecDeque::new(),
            next_activity: 0,
            peer: None,
            control: None,
            sync: None,
            own_name: String::new(),
            server: None,
            tasks: Vec::new(),
        }
    }
}

impl Inner {
    fn reset(&mut self) {
        self.generation += 1;
        if let Some(peer) = self.peer.take() {
            peer.close();
        }
        self.control = None;
        self.sync = None;
        for task in self.tasks.drain(..) {
            task.abort();
        }
        self.view = SessionView::Idle;
    }

    fn log(&mut self, text: impl Into<String>, tone: Tone) {
        self.next_activity += 1;
        self.activity.push_back(Activity {
            id: self.next_activity,
            text: text.into(),
            tone,
        });
        while self.activity.len() > ACTIVITY_LIMIT {
            self.activity.pop_front();
        }
    }

    fn guest_ids(&self) -> Vec<PeerId> {
        match &self.view {
            SessionView::Hosting { guests, .. } => guests.iter().map(|person| person.id).collect(),
            _ => Vec::new(),
        }
    }

    fn name_of(&self, peer: PeerId) -> String {
        match &self.view {
            SessionView::Hosting { guests, requests, .. } => guests
                .iter()
                .chain(requests)
                .find(|person| person.id == peer)
                .map(|person| person.name.clone())
                .unwrap_or_else(|| format!("Guest {peer}")),
            SessionView::Joined { host_name, .. } if peer == HOST => host_name.clone(),
            _ => format!("Guest {peer}"),
        }
    }
}

fn game_state(server: Option<SteamApp>) -> GameState {
    GameState {
        running: server.is_some(),
        steam_app: server.unwrap_or_default().argument().to_owned(),
    }
}

#[derive(Clone, Default)]
pub struct SessionState {
    inner: Arc<Mutex<Inner>>,
}

impl SessionState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn shutdown(&self) {
        self.lock().reset();
    }

    pub fn server_changed(&self, app: &AppHandle, server: Option<SteamApp>) {
        let mut inner = self.lock();
        if inner.server == server {
            return;
        }
        inner.server = server;
        if let Some(sync) = &inner.sync {
            sync.set_game(game_state(server));
        }
        if let SessionView::Hosting { game_running, .. } = &mut inner.view {
            *game_running = server.is_some();
            let text = if server.is_some() { "Your test server runs, guests can join the game." } else { "Your test server stopped." };
            inner.log(text, Tone::Info);
            publish(app, &inner, None);
        }
    }
}

fn settings_file(app: &AppHandle) -> CommandResult<PathBuf> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| CommandError::new("Settings folder unavailable", error.to_string()))?;
    Ok(directory.join(SETTINGS_FILE))
}

fn load_settings(app: &AppHandle) -> SessionSettings {
    settings_file(app)
        .ok()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|content| toml::from_str(&content).ok())
        .unwrap_or_default()
}

fn status_of(app: &AppHandle, inner: &Inner) -> SessionStatus {
    SessionStatus {
        settings: load_settings(app),
        view: inner.view.clone(),
        activity: inner.activity.iter().cloned().collect(),
    }
}

fn publish(app: &AppHandle, inner: &Inner, notice: Option<Notice>) {
    let _ = app.emit(SESSION_EVENT, SessionMessage { status: status_of(app, inner), notice });
}

fn notice(title: &str, message: impl Into<String>, tone: Tone) -> Option<Notice> {
    Some(Notice {
        title: title.to_owned(),
        message: message.into(),
        tone,
    })
}

fn relay_from(settings: &SessionSettings) -> CommandResult<RelayAddress> {
    if settings.relay.trim().is_empty() {
        return Err(CommandError::new("No relay yet", "Enter the address of your NanosFlow relay in the session settings first."));
    }
    settings.relay.parse().map_err(|error: nf_session::SessionError| CommandError::new("Relay address not valid", error.to_string()))
}

fn display_name(settings: &SessionSettings, fallback: &str) -> String {
    let name = settings.name.trim();
    if name.is_empty() { fallback.to_owned() } else { name.to_owned() }
}

fn ensure_idle(inner: &Inner) -> CommandResult<()> {
    if inner.view == SessionView::Idle {
        Ok(())
    } else {
        Err(CommandError::new("Already in a session", "Leave the current session first."))
    }
}

#[tauri::command]
pub fn session_status(app: AppHandle, state: State<'_, SessionState>) -> SessionStatus {
    status_of(&app, &state.lock())
}

#[tauri::command]
pub fn update_session_settings(app: AppHandle, state: State<'_, SessionState>, settings: SessionSettings) -> CommandResult<SessionStatus> {
    let settings = SessionSettings {
        relay: settings.relay.trim().to_owned(),
        name: settings.name.trim().to_owned(),
        guest_port: settings.guest_port,
    };
    if !settings.relay.is_empty() {
        relay_from(&settings)?;
    }
    if settings.name.chars().count() > MAX_NAME_LENGTH {
        return Err(CommandError::new("Name too long", format!("Keep your name to {MAX_NAME_LENGTH} characters.")));
    }
    if settings.guest_port < 1024 {
        return Err(CommandError::new("Port not allowed", "Pick a port between 1024 and 65535."));
    }
    let file = settings_file(&app)?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    }
    let content = toml::to_string(&settings).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    std::fs::write(&file, content).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    let inner = state.lock();
    publish(&app, &inner, None);
    Ok(status_of(&app, &inner))
}

fn describe_game_event(inner: &Inner, event: &GameEvent) -> Option<(String, Tone)> {
    match event {
        GameEvent::PlayerOpened { peer, .. } => Some((format!("{} connected to the game.", inner.name_of(*peer)), Tone::Success)),
        GameEvent::PlayerClosed { peer, traffic, reason, .. } => {
            let tone = if reason == "idle" { Tone::Info } else { Tone::Warning };
            let reason = if reason == "idle" { "no traffic for a minute".to_owned() } else { reason.clone() };
            Some((format!("Game connection of {} closed ({reason}, {traffic}).", inner.name_of(*peer)), tone))
        }
        GameEvent::DownloadOpened { .. } => None,
        GameEvent::DownloadClosed { peer, traffic, error: None } => Some((format!("{} downloaded game files ({traffic}).", inner.name_of(*peer)), Tone::Info)),
        GameEvent::DownloadClosed { peer, error: Some(error), .. } => Some((format!("A download of {} failed: {error}", inner.name_of(*peer)), Tone::Warning)),
    }
}

fn watch_game_events(app: AppHandle, state: SessionState, generation: u64, mut events: mpsc::UnboundedReceiver<GameEvent>) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            let mut inner = state.lock();
            if inner.generation != generation {
                return;
            }
            if let Some((text, tone)) = describe_game_event(&inner, &event) {
                inner.log(text, tone);
                publish(&app, &inner, None);
            }
        }
    })
}

fn end(app: &AppHandle, state: &SessionState, generation: u64, title: &str, message: String) {
    let mut inner = state.lock();
    if inner.generation != generation {
        return;
    }
    inner.reset();
    inner.log(message.clone(), Tone::Warning);
    publish(app, &inner, notice(title, message, Tone::Warning));
}

fn send_presence(peer: &PeerConnection, to: PeerId, presence: &Presence) {
    if let Ok(encoded) = serde_json::to_vec(presence) {
        peer.send_datagram(to, Channel::Presence, &encoded);
    }
}

fn watch_sync_events(app: AppHandle, state: SessionState, generation: u64, project_root: PathBuf, mut events: mpsc::UnboundedReceiver<SyncEvent>) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            let mut inner = state.lock();
            if inner.generation != generation {
                return;
            }
            match event {
                SyncEvent::Applied { paths, author } => {
                    let author = inner.name_of(author);
                    let _ = app.emit(
                        SESSION_FILES_EVENT,
                        FilesMessage {
                            project_root: project_root.to_string_lossy().into_owned(),
                            paths,
                            author,
                        },
                    );
                }
                SyncEvent::Shared { .. } => {}
                SyncEvent::Game { state } => {
                    let SessionView::Joined { game, host_name, .. } = &mut inner.view else {
                        continue;
                    };
                    if *game == state {
                        continue;
                    }
                    let host_name = host_name.clone();
                    *game = state.clone();
                    let announcement = if state.running {
                        inner.log(format!("{host_name} started the test server."), Tone::Success);
                        notice("Game ready", format!("{host_name} started the test server. Press Join game to play."), Tone::Success)
                    } else {
                        inner.log(format!("{host_name} stopped the test server."), Tone::Info);
                        None
                    };
                    publish(&app, &inner, announcement);
                }
                SyncEvent::Failed { message } => {
                    inner.log(message, Tone::Warning);
                    publish(&app, &inner, None);
                }
            }
        }
    })
}

fn relay_guest_presence(app: AppHandle, state: SessionState, generation: u64, mut datagrams: mpsc::UnboundedReceiver<nf_session::Datagram>) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        while let Some(datagram) = datagrams.recv().await {
            let Ok(update) = serde_json::from_slice::<PresenceUpdate>(&datagram.payload) else {
                continue;
            };
            let inner = state.lock();
            if inner.generation != generation {
                return;
            }
            let Some(peer) = inner.peer.clone() else {
                continue;
            };
            let presence = Presence {
                peer: datagram.peer,
                name: inner.name_of(datagram.peer),
                graph: update.graph,
                x: update.x,
                y: update.y,
                selection: update.selection.into_iter().take(MAX_SELECTION).collect(),
                gone: false,
            };
            for guest in inner.guest_ids().into_iter().filter(|guest| *guest != datagram.peer) {
                send_presence(&peer, guest, &presence);
            }
            let _ = app.emit(SESSION_PRESENCE_EVENT, presence);
        }
    })
}

fn receive_host_presence(app: AppHandle, mut datagrams: mpsc::UnboundedReceiver<nf_session::Datagram>) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        while let Some(datagram) = datagrams.recv().await {
            if let Ok(presence) = serde_json::from_slice::<Presence>(&datagram.payload) {
                let _ = app.emit(SESSION_PRESENCE_EVENT, presence);
            }
        }
    })
}

#[tauri::command]
pub fn update_presence(state: State<'_, SessionState>, presence: PresenceUpdate) {
    let inner = state.lock();
    let Some(peer) = inner.peer.clone() else {
        return;
    };
    match &inner.view {
        SessionView::Hosting { guests, .. } => {
            let shared = Presence {
                peer: HOST,
                name: inner.own_name.clone(),
                graph: presence.graph,
                x: presence.x,
                y: presence.y,
                selection: presence.selection.into_iter().take(MAX_SELECTION).collect(),
                gone: false,
            };
            for guest in guests {
                send_presence(&peer, guest.id, &shared);
            }
        }
        SessionView::Joined { .. } => {
            let update = PresenceUpdate {
                selection: presence.selection.into_iter().take(MAX_SELECTION).collect(),
                ..presence
            };
            if let Ok(encoded) = serde_json::to_vec(&update) {
                peer.send_datagram(HOST, Channel::Presence, &encoded);
            }
        }
        _ => {}
    }
}

fn session_workspace(app: &AppHandle, code: &str) -> CommandResult<PathBuf> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| CommandError::new("No data folder", format!("NanosFlow could not find its data folder: {error}")))?;
    Ok(directory.join(SESSIONS_DIRECTORY).join(code))
}

pub fn is_session_copy(app: &AppHandle, path: &std::path::Path) -> bool {
    app.path()
        .app_data_dir()
        .is_ok_and(|directory| path.starts_with(directory.join(SESSIONS_DIRECTORY)))
}

#[tauri::command]
pub fn host_session(app: AppHandle, state: State<'_, SessionState>, project_file: PathBuf) -> CommandResult<SessionStatus> {
    let settings = load_settings(&app);
    let relay = relay_from(&settings)?;
    let project = Project::open(&project_file)?;
    let server = SocketAddr::from(([127, 0, 0, 1], project.local_settings().test_server.port));
    let project_root = project.root().to_path_buf();
    let project_name = project
        .project_file()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = display_name(&settings, "Host");
    let generation = {
        let mut inner = state.lock();
        ensure_idle(&inner)?;
        inner.reset();
        inner.view = SessionView::Starting;
        inner.log(format!("Opening a session on {}:{}...", relay.host, relay.port), Tone::Info);
        publish(&app, &inner, None);
        inner.generation
    };
    let state = state.inner().clone();
    let task = tauri::async_runtime::spawn({
        let state = state.clone();
        let app = app.clone();
        async move {
            let hosted = match nf_session::host(&relay, &name).await {
                Ok(hosted) => hosted,
                Err(error) => return end(&app, &state, generation, "Session not started", error.to_string()),
            };
            let (game_events, game_received) = mpsc::unbounded_channel();
            let game = tauri::async_runtime::spawn(nf_session::serve_game(
                hosted.peer.clone(),
                hosted.inbox.game_datagrams,
                hosted.inbox.game_streams,
                server,
                IDLE_TIMEOUT,
                game_events,
            ));
            let watcher = watch_game_events(app.clone(), state.clone(), generation, game_received);
            let (sync_events, sync_received) = mpsc::unbounded_channel();
            let sync = match nf_session::host_sync(project_root.clone(), project_name, hosted.inbox.sync_streams, sync_events) {
                Ok(sync) => sync,
                Err(error) => {
                    hosted.peer.close();
                    game.abort();
                    watcher.abort();
                    return end(&app, &state, generation, "Session not started", error.to_string());
                }
            };
            let sync_watcher = watch_sync_events(app.clone(), state.clone(), generation, project_root.clone(), sync_received);
            let presence = relay_guest_presence(app.clone(), state.clone(), generation, hosted.inbox.presence);
            {
                let mut inner = state.lock();
                if inner.generation != generation {
                    hosted.peer.close();
                    for task in [game, watcher, sync_watcher, presence] {
                        task.abort();
                    }
                    return;
                }
                inner.view = SessionView::Hosting {
                    code: display_code(&hosted.code),
                    server: server.to_string(),
                    project_file: project_file.to_string_lossy().into_owned(),
                    game_running: inner.server.is_some(),
                    guests: Vec::new(),
                    requests: Vec::new(),
                };
                inner.peer = Some(hosted.peer.clone());
                inner.control = Some(hosted.control.clone());
                sync.set_game(game_state(inner.server));
                inner.sync = Some(sync);
                inner.own_name = name.clone();
                inner.tasks.extend([game, watcher, sync_watcher, presence]);
                let code = display_code(&hosted.code);
                inner.log(format!("Session {code} is open. Share the code with your guests."), Tone::Success);
                publish(&app, &inner, None);
            }
            let mut events = hosted.events;
            while let Some(event) = events.recv().await {
                let mut inner = state.lock();
                if inner.generation != generation {
                    return;
                }
                let mut announcement = None;
                match event {
                    HostEvent::JoinRequested { guest, name } => {
                        inner.log(format!("{name} asks to join."), Tone::Info);
                        announcement = notice("Join request", format!("{name} wants to join your session."), Tone::Info);
                        if let SessionView::Hosting { requests, .. } = &mut inner.view {
                            requests.push(Person { id: guest, name });
                        }
                    }
                    HostEvent::GuestLeft { guest } => {
                        let name = inner.name_of(guest);
                        let _ = app.emit(SESSION_PRESENCE_EVENT, Presence::gone(guest));
                        if let Some(peer) = inner.peer.clone() {
                            for other in inner.guest_ids().into_iter().filter(|other| *other != guest) {
                                send_presence(&peer, other, &Presence::gone(guest));
                            }
                        }
                        if let SessionView::Hosting { guests, requests, .. } = &mut inner.view {
                            let was_guest = guests.iter().any(|person| person.id == guest);
                            guests.retain(|person| person.id != guest);
                            requests.retain(|person| person.id != guest);
                            let text = if was_guest { format!("{name} left the session.") } else { format!("{name} withdrew the request.") };
                            inner.log(text, Tone::Info);
                        }
                    }
                    HostEvent::Ended { reason } => {
                        drop(inner);
                        return end(&app, &state, generation, "Session ended", reason);
                    }
                }
                publish(&app, &inner, announcement);
            }
        }
    });
    let mut inner = state.lock();
    if inner.generation == generation {
        inner.tasks.push(task);
    }
    Ok(status_of(&app, &inner))
}

#[tauri::command]
pub fn answer_join_request(app: AppHandle, state: State<'_, SessionState>, guest: PeerId, admit: bool) -> CommandResult<SessionStatus> {
    let mut inner = state.lock();
    let Some(control) = inner.control.clone() else {
        return Err(CommandError::new("Not hosting", "There is no session to answer for."));
    };
    let SessionView::Hosting { guests, requests, .. } = &mut inner.view else {
        return Err(CommandError::new("Not hosting", "There is no session to answer for."));
    };
    let Some(position) = requests.iter().position(|person| person.id == guest) else {
        return Err(CommandError::new("Request gone", "This guest is no longer waiting."));
    };
    let person = requests.remove(position);
    let name = person.name.clone();
    if admit {
        control.admit(guest);
        guests.push(person);
        inner.log(format!("{name} joined the session."), Tone::Success);
    } else {
        control.refuse(guest, "The host declined your request.");
        inner.log(format!("You declined {name}."), Tone::Info);
    }
    publish(&app, &inner, None);
    Ok(status_of(&app, &inner))
}

#[tauri::command]
pub fn kick_guest(app: AppHandle, state: State<'_, SessionState>, guest: PeerId) -> CommandResult<SessionStatus> {
    let mut inner = state.lock();
    let Some(control) = inner.control.clone() else {
        return Err(CommandError::new("Not hosting", "There is no session to remove guests from."));
    };
    let name = inner.name_of(guest);
    if let SessionView::Hosting { guests, .. } = &mut inner.view {
        guests.retain(|person| person.id != guest);
    }
    control.kick(guest);
    inner.log(format!("You removed {name} from the session."), Tone::Info);
    publish(&app, &inner, None);
    Ok(status_of(&app, &inner))
}

async fn bind_entrance(port: u16) -> std::io::Result<GameEntrance> {
    match GameEntrance::bind(SocketAddr::from(([127, 0, 0, 1], port))).await {
        Ok(entrance) => Ok(entrance),
        Err(_) => GameEntrance::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await,
    }
}

#[tauri::command]
pub fn join_session(app: AppHandle, state: State<'_, SessionState>, code: String) -> CommandResult<SessionStatus> {
    let settings = load_settings(&app);
    let relay = relay_from(&settings)?;
    let normalized = nf_session::normalize_code(&code);
    if normalized.len() != nf_session::CODE_LENGTH {
        return Err(CommandError::new("Code not valid", format!("A session code has {} letters and digits, like K7Q-M2X.", nf_session::CODE_LENGTH)));
    }
    let name = display_name(&settings, "Guest");
    let workspace = session_workspace(&app, &normalized)?;
    let generation = {
        let mut inner = state.lock();
        ensure_idle(&inner)?;
        inner.reset();
        inner.view = SessionView::Waiting { code: display_code(&normalized) };
        inner.log(format!("Asking to join {}...", display_code(&normalized)), Tone::Info);
        publish(&app, &inner, None);
        inner.generation
    };
    let state = state.inner().clone();
    let task = tauri::async_runtime::spawn({
        let state = state.clone();
        let app = app.clone();
        async move {
            let entrance = match bind_entrance(settings.guest_port).await {
                Ok(entrance) => entrance,
                Err(error) => return end(&app, &state, generation, "Could not join", format!("No local port for the game is free: {error}")),
            };
            let address = entrance.local_address().map(|address| address.to_string()).unwrap_or_default();
            let joined = match nf_session::join(&relay, &normalized, &name).await {
                Ok(joined) => joined,
                Err(error) => return end(&app, &state, generation, "Could not join", error.to_string()),
            };
            let (game_events, game_received) = mpsc::unbounded_channel();
            let game = tauri::async_runtime::spawn(entrance.run(joined.peer.clone(), joined.inbox.game_datagrams, IDLE_TIMEOUT, game_events));
            let watcher = watch_game_events(app.clone(), state.clone(), generation, game_received);
            let (sync_events, sync_received) = mpsc::unbounded_channel();
            let (sync, project_file) = match nf_session::guest_sync(workspace.clone(), &joined.peer, sync_events).await {
                Ok(synced) => synced,
                Err(error) => {
                    joined.peer.close();
                    game.abort();
                    watcher.abort();
                    return end(&app, &state, generation, "Could not join", error.to_string());
                }
            };
            let sync_watcher = watch_sync_events(app.clone(), state.clone(), generation, workspace.clone(), sync_received);
            let presence = receive_host_presence(app.clone(), joined.inbox.presence);
            {
                let mut inner = state.lock();
                if inner.generation != generation {
                    joined.peer.close();
                    for task in [game, watcher, sync_watcher, presence] {
                        task.abort();
                    }
                    return;
                }
                inner.view = SessionView::Joined {
                    host_name: joined.host_name.clone(),
                    address: address.clone(),
                    project_file: project_file.to_string_lossy().into_owned(),
                    game: GameState::default(),
                };
                inner.peer = Some(joined.peer.clone());
                inner.sync = Some(sync);
                inner.own_name = name.clone();
                inner.tasks.extend([game, watcher, sync_watcher, presence]);
                inner.log(format!("You joined the session of {}.", joined.host_name), Tone::Success);
                publish(
                    &app,
                    &inner,
                    notice("Joined", format!("Connect nanos world with Direct Connect to {address}."), Tone::Success),
                );
            }
            let reason = joined.peer.closed().await;
            end(&app, &state, generation, "Session ended", reason);
        }
    });
    let mut inner = state.lock();
    if inner.generation == generation {
        inner.tasks.push(task);
    }
    Ok(status_of(&app, &inner))
}

#[tauri::command]
pub fn join_session_game(app: AppHandle, state: State<'_, SessionState>) -> CommandResult<String> {
    let (address, game) = match &state.lock().view {
        SessionView::Joined { address, game, .. } => (address.clone(), game.clone()),
        _ => return Err(CommandError::new("Not in a session", "Join a session first.")),
    };
    if !game.running {
        return Err(CommandError::new("Game not ready", "The host has not started the test server yet."));
    }
    let steam_app = if game.steam_app == SteamApp::Game.argument() { SteamApp::Game } else { SteamApp::Playtest };
    let clients = nf_server::installed_clients(&nf_server::steam_roots());
    let Some(client) = nf_server::choose_client(&clients, steam_app) else {
        let wanted = if steam_app == SteamApp::Game { "nanos world" } else { "nanos world Playtest" };
        return Err(CommandError::new(
            "nanos world not found",
            format!("The host runs the server for {wanted}, but it is not installed in Steam on this computer. Connect manually with Direct Connect to {address}."),
        ));
    };
    app.opener()
        .open_url(nf_server::connect_url(client.app_id, &address), None::<&str>)
        .map_err(|error| CommandError::new("Steam did not open", format!("Steam could not be started: {error}. Connect manually with Direct Connect to {address}.")))?;
    Ok(format!("Starting {} and connecting to {address}.", client.name))
}

#[tauri::command]
pub fn leave_session(app: AppHandle, state: State<'_, SessionState>) -> SessionStatus {
    let mut inner = state.lock();
    if inner.view != SessionView::Idle {
        let hosting = matches!(inner.view, SessionView::Hosting { .. } | SessionView::Starting);
        inner.reset();
        inner.log(if hosting { "You ended the session." } else { "You left the session." }, Tone::Info);
        publish(&app, &inner, None);
    }
    status_of(&app, &inner)
}
