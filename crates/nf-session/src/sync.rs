use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use nf_relay::protocol::{Channel, HOST, PeerId, read_message_limited, write_message_limited};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use quinn::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::SessionError;
use crate::peer::{IncomingStream, PeerConnection};

pub const PACKAGES_DIRECTORY: &str = "packages";
pub const PROJECT_EXTENSION: &str = "nfproj";
const MAX_FILE_LENGTH: u64 = 1024 * 1024;
const MAX_SYNC_MESSAGE: usize = 64 * 1024 * 1024;
const DEBOUNCE: Duration = Duration::from_millis(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameState {
    pub running: bool,
    pub steam_app: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum SyncMessage {
    Snapshot { project_file: String, files: Vec<FileChange> },
    Changes { changes: Vec<FileChange>, echo: bool },
    Game { state: GameState },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    Applied { paths: Vec<String>, author: PeerId },
    Shared { paths: Vec<String> },
    Game { state: GameState },
    Failed { message: String },
}

pub fn is_syncable(relative: &str) -> bool {
    let path = Path::new(relative);
    if relative.is_empty() || !path.components().all(|component| matches!(component, Component::Normal(_))) {
        return false;
    }
    let parts: Vec<&str> = relative.split('/').collect();
    match parts.as_slice() {
        [file] => Path::new(file).extension().is_some_and(|extension| extension == PROJECT_EXTENSION),
        [first, rest @ ..] => *first == PACKAGES_DIRECTORY && !rest.is_empty() && rest.iter().all(|part| !part.is_empty() && !part.starts_with('.')),
        [] => false,
    }
}

fn relative_of(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = relative.components().map(|component| component.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

fn read_text(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_LENGTH {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn files_below(root: &Path, directory: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_below(root, &path, found);
        } else if let Some(relative) = relative_of(root, &path).filter(|relative| is_syncable(relative)) {
            found.push(relative);
        }
    }
}

pub struct Mirror {
    root: PathBuf,
    known: HashMap<String, String>,
}

impl Mirror {
    pub fn new(root: PathBuf) -> Self {
        Self { root, known: HashMap::new() }
    }

    pub fn scan_all(&mut self) -> Vec<FileChange> {
        let mut found = Vec::new();
        files_below(&self.root, &self.root, &mut found);
        found.sort();
        self.known.clear();
        found
            .into_iter()
            .filter_map(|relative| {
                let content = read_text(&self.root.join(&relative))?;
                self.known.insert(relative.clone(), content.clone());
                Some(FileChange { path: relative, content: Some(content) })
            })
            .collect()
    }

    pub fn snapshot(&self) -> Vec<FileChange> {
        let mut files: Vec<FileChange> = self
            .known
            .iter()
            .map(|(path, content)| FileChange {
                path: path.clone(),
                content: Some(content.clone()),
            })
            .collect();
        files.sort_by(|first, second| first.path.cmp(&second.path));
        files
    }

    pub fn detect(&mut self, paths: &[PathBuf]) -> Vec<FileChange> {
        let mut candidates = BTreeSet::new();
        for path in paths {
            let Some(relative) = relative_of(&self.root, path) else {
                continue;
            };
            if path.is_dir() {
                let mut found = Vec::new();
                files_below(&self.root, path, &mut found);
                candidates.extend(found);
            }
            let prefix = format!("{relative}/");
            candidates.extend(self.known.keys().filter(|known| known.starts_with(&prefix)).cloned());
            if is_syncable(&relative) {
                candidates.insert(relative);
            }
        }
        let mut changes = Vec::new();
        for relative in candidates {
            let current = read_text(&self.root.join(&relative));
            if self.known.get(&relative) == current.as_ref() {
                continue;
            }
            match &current {
                Some(content) => self.known.insert(relative.clone(), content.clone()),
                None => self.known.remove(&relative),
            };
            changes.push(FileChange { path: relative, content: current });
        }
        changes
    }

    pub fn apply(&mut self, changes: &[FileChange]) -> Vec<String> {
        let mut applied = Vec::new();
        for change in changes {
            if !is_syncable(&change.path) || self.known.get(&change.path) == change.content.as_ref() {
                continue;
            }
            let path = self.root.join(&change.path);
            let written = match &change.content {
                Some(content) => path
                    .parent()
                    .map_or(Ok(()), std::fs::create_dir_all)
                    .and_then(|()| std::fs::write(&path, content))
                    .is_ok(),
                None => {
                    let removed = std::fs::remove_file(&path).is_ok() || !path.exists();
                    if removed {
                        remove_empty_parents(&self.root, &path);
                    }
                    removed
                }
            };
            if !written {
                continue;
            }
            match &change.content {
                Some(content) => self.known.insert(change.path.clone(), content.clone()),
                None => self.known.remove(&change.path),
            };
            applied.push(change.path.clone());
        }
        applied
    }

    pub fn replace_with(&mut self, files: &[FileChange]) -> Vec<String> {
        let mut present = Vec::new();
        files_below(&self.root, &self.root, &mut present);
        let incoming: BTreeSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
        self.known.clear();
        for stale in present.iter().filter(|path| !incoming.contains(path.as_str())) {
            let path = self.root.join(stale);
            let _ = std::fs::remove_file(&path);
            remove_empty_parents(&self.root, &path);
        }
        for relative in &present {
            if let Some(content) = read_text(&self.root.join(relative)) {
                self.known.insert(relative.clone(), content);
            }
        }
        self.apply(files)
    }
}

fn remove_empty_parents(root: &Path, path: &Path) {
    let mut current = path.parent();
    while let Some(directory) = current {
        if directory == root || std::fs::remove_dir(directory).is_err() {
            return;
        }
        current = directory.parent();
    }
}

type SharedMirror = Arc<Mutex<Mirror>>;

fn lock(mirror: &SharedMirror) -> MutexGuard<'_, Mirror> {
    mirror.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn watch(root: &Path) -> Result<(RecommendedWatcher, UnboundedReceiver<Vec<PathBuf>>), SessionError> {
    let (raw_sender, mut raw) = mpsc::unbounded_channel::<Vec<PathBuf>>();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && !event.kind.is_access()
        {
            let _ = raw_sender.send(event.paths);
        }
    })
    .map_err(|error| SessionError::Sync(format!("The project folder cannot be watched: {error}")))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|error| SessionError::Sync(format!("The project folder cannot be watched: {error}")))?;
    let (batches, received) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(first) = raw.recv().await {
            let mut paths: BTreeSet<PathBuf> = first.into_iter().collect();
            let deadline = tokio::time::sleep(DEBOUNCE);
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    more = raw.recv() => match more {
                        Some(more) => paths.extend(more),
                        None => break,
                    },
                    _ = &mut deadline => break,
                }
            }
            if batches.send(paths.into_iter().collect()).is_err() {
                return;
            }
        }
    });
    Ok((watcher, received))
}

pub struct SyncHandle {
    tasks: Vec<JoinHandle<()>>,
    _watcher: RecommendedWatcher,
    host: Option<HostShared>,
}

#[derive(Clone)]
struct HostShared {
    guests: Arc<Mutex<Guests>>,
    game: Arc<Mutex<GameState>>,
}

impl SyncHandle {
    pub fn set_game(&self, state: GameState) {
        let Some(host) = &self.host else {
            return;
        };
        let mut game = host.game.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if *game == state {
            return;
        }
        *game = state.clone();
        let mut guests = host.guests.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        guests.writers.retain(|_, writer| writer.send(SyncMessage::Game { state: state.clone() }).is_ok());
    }
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn send_message(stream: &mut SendStream, message: &SyncMessage) -> bool {
    write_message_limited(stream, message, MAX_SYNC_MESSAGE).await.is_ok()
}

async fn read_sync(stream: &mut RecvStream) -> Option<SyncMessage> {
    read_message_limited(stream, MAX_SYNC_MESSAGE).await.ok().flatten()
}

fn spawn_writer(mut stream: SendStream, mut messages: UnboundedReceiver<SyncMessage>) {
    tokio::spawn(async move {
        while let Some(message) = messages.recv().await {
            if !send_message(&mut stream, &message).await {
                return;
            }
        }
    });
}

#[derive(Default)]
struct Guests {
    writers: HashMap<PeerId, UnboundedSender<SyncMessage>>,
}

impl Guests {
    fn broadcast(&mut self, changes: &[FileChange], origin: Option<PeerId>) {
        self.writers.retain(|guest, writer| {
            writer
                .send(SyncMessage::Changes {
                    changes: changes.to_vec(),
                    echo: Some(*guest) == origin,
                })
                .is_ok()
        });
    }
}

pub fn host_sync(
    root: PathBuf,
    project_file: String,
    mut streams: UnboundedReceiver<IncomingStream>,
    events: UnboundedSender<SyncEvent>,
) -> Result<SyncHandle, SessionError> {
    let mut mirror = Mirror::new(root.clone());
    mirror.scan_all();
    let mirror: SharedMirror = Arc::new(Mutex::new(mirror));
    let guests: Arc<Mutex<Guests>> = Arc::default();
    let game: Arc<Mutex<GameState>> = Arc::default();
    let shared = HostShared {
        guests: guests.clone(),
        game: game.clone(),
    };
    let (watcher, mut batches) = watch(&root)?;

    let local = tokio::spawn({
        let mirror = mirror.clone();
        let guests = guests.clone();
        let events = events.clone();
        async move {
            while let Some(paths) = batches.recv().await {
                let mut mirror = lock(&mirror);
                let changes = mirror.detect(&paths);
                if changes.is_empty() {
                    continue;
                }
                guests.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).broadcast(&changes, None);
                let _ = events.send(SyncEvent::Shared {
                    paths: changes.into_iter().map(|change| change.path).collect(),
                });
            }
        }
    });

    let accept = tokio::spawn(async move {
        while let Some(stream) = streams.recv().await {
            let guest = stream.peer;
            let (writer, outgoing) = mpsc::unbounded_channel();
            {
                let mirror = lock(&mirror);
                let _ = writer.send(SyncMessage::Snapshot {
                    project_file: project_file.clone(),
                    files: mirror.snapshot(),
                });
                let state = game.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
                let _ = writer.send(SyncMessage::Game { state });
                guests.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).writers.insert(guest, writer);
            }
            spawn_writer(stream.send, outgoing);
            let mirror = mirror.clone();
            let guests = guests.clone();
            let events = events.clone();
            let mut receive = stream.receive;
            tokio::spawn(async move {
                while let Some(message) = read_sync(&mut receive).await {
                    let SyncMessage::Changes { changes, .. } = message else {
                        continue;
                    };
                    let mut mirror = lock(&mirror);
                    let applied = mirror.apply(&changes);
                    let accepted: Vec<FileChange> = changes.into_iter().filter(|change| is_syncable(&change.path)).collect();
                    if accepted.is_empty() {
                        continue;
                    }
                    guests.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).broadcast(&accepted, Some(guest));
                    if !applied.is_empty() {
                        let _ = events.send(SyncEvent::Applied { paths: applied, author: guest });
                    }
                }
                guests.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).writers.remove(&guest);
            });
        }
    });

    Ok(SyncHandle {
        tasks: vec![local, accept],
        _watcher: watcher,
        host: Some(shared),
    })
}

#[derive(Default)]
struct InFlight {
    counts: HashMap<String, usize>,
}

impl InFlight {
    fn sent(&mut self, changes: &[FileChange]) {
        for change in changes {
            *self.counts.entry(change.path.clone()).or_default() += 1;
        }
    }

    fn accept_echo(&mut self, path: &str) -> bool {
        match self.counts.get_mut(path) {
            Some(count) if *count > 1 => {
                *count -= 1;
                false
            }
            Some(_) => {
                self.counts.remove(path);
                true
            }
            None => true,
        }
    }

    fn is_pending(&self, path: &str) -> bool {
        self.counts.contains_key(path)
    }
}

pub async fn guest_sync(workspace: PathBuf, peer: &PeerConnection, events: UnboundedSender<SyncEvent>) -> Result<(SyncHandle, PathBuf), SessionError> {
    let (mut send, mut receive) = peer.open_stream(HOST, Channel::Sync).await?;
    send_message(&mut send, &SyncMessage::Changes { changes: Vec::new(), echo: false }).await;
    let Some(SyncMessage::Snapshot { project_file, files }) = read_sync(&mut receive).await else {
        return Err(SessionError::Sync("The host did not send the project.".to_owned()));
    };
    if !is_syncable(&project_file) || !project_file.contains('.') {
        return Err(SessionError::Sync("The host sent an invalid project.".to_owned()));
    }
    std::fs::create_dir_all(&workspace).map_err(|error| SessionError::Sync(format!("The session folder cannot be created: {error}")))?;
    let mut mirror = Mirror::new(workspace.clone());
    mirror.replace_with(&files);
    let mirror: SharedMirror = Arc::new(Mutex::new(mirror));
    let in_flight: Arc<Mutex<InFlight>> = Arc::default();
    let (watcher, mut batches) = watch(&workspace)?;
    let (writer, outgoing) = mpsc::unbounded_channel();
    spawn_writer(send, outgoing);

    let local = tokio::spawn({
        let mirror = mirror.clone();
        let in_flight = in_flight.clone();
        let events = events.clone();
        async move {
            while let Some(paths) = batches.recv().await {
                let changes = lock(&mirror).detect(&paths);
                if changes.is_empty() {
                    continue;
                }
                in_flight.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).sent(&changes);
                let paths = changes.iter().map(|change| change.path.clone()).collect();
                if writer.send(SyncMessage::Changes { changes, echo: false }).is_err() {
                    return;
                }
                let _ = events.send(SyncEvent::Shared { paths });
            }
        }
    });

    let remote = tokio::spawn(async move {
        while let Some(message) = read_sync(&mut receive).await {
            let (changes, echo) = match message {
                SyncMessage::Changes { changes, echo } => (changes, echo),
                SyncMessage::Game { state } => {
                    let _ = events.send(SyncEvent::Game { state });
                    continue;
                }
                SyncMessage::Snapshot { .. } => continue,
            };
            let accepted: Vec<FileChange> = {
                let mut in_flight = in_flight.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                changes
                    .into_iter()
                    .filter(|change| if echo { in_flight.accept_echo(&change.path) } else { !in_flight.is_pending(&change.path) })
                    .collect()
            };
            let applied = lock(&mirror).apply(&accepted);
            if !applied.is_empty() {
                let _ = events.send(SyncEvent::Applied { paths: applied, author: HOST });
            }
        }
        let _ = events.send(SyncEvent::Failed {
            message: "The project connection to the host closed.".to_owned(),
        });
    });

    Ok((
        SyncHandle {
            tasks: vec![local, remote],
            _watcher: watcher,
            host: None,
        },
        workspace.join(project_file),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_project_files() {
        assert!(is_syncable("Arena.nfproj"));
        assert!(is_syncable("packages/arena/graphs/server/Main/nodes/01J.toml"));
        assert!(is_syncable("packages/arena/package.toml"));
        assert!(!is_syncable(".nanosflow/settings.toml"));
        assert!(!is_syncable("build/arena/Server/Index.lua"));
        assert!(!is_syncable("packages/../escape.toml"));
        assert!(!is_syncable("packages/arena/.git/config"));
        assert!(!is_syncable("/etc/passwd"));
        assert!(!is_syncable("packages"));
    }

    #[test]
    fn detects_applies_and_replaces_files() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().to_path_buf();
        std::fs::write(root.join("Game.nfproj"), "name = 1").unwrap();
        std::fs::create_dir_all(root.join("packages/game/graphs")).unwrap();
        std::fs::write(root.join("packages/game/graphs/a.toml"), "a").unwrap();
        std::fs::create_dir_all(root.join(".nanosflow")).unwrap();
        std::fs::write(root.join(".nanosflow/settings.toml"), "local").unwrap();
        let mut mirror = Mirror::new(root.clone());
        assert_eq!(mirror.scan_all().len(), 2);

        std::fs::write(root.join("packages/game/graphs/a.toml"), "changed").unwrap();
        std::fs::write(root.join("packages/game/graphs/b.toml"), "new").unwrap();
        let detected = mirror.detect(&[root.join("packages/game/graphs")]);
        assert_eq!(detected.iter().map(|change| change.path.as_str()).collect::<Vec<_>>(), ["packages/game/graphs/a.toml", "packages/game/graphs/b.toml"]);
        assert!(mirror.detect(&[root.join("packages/game/graphs/a.toml")]).is_empty());

        std::fs::remove_dir_all(root.join("packages/game/graphs")).unwrap();
        let removed = mirror.detect(&[root.join("packages/game/graphs")]);
        assert!(removed.iter().all(|change| change.content.is_none()) && removed.len() == 2);

        let other = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(other.path().join("packages/old")).unwrap();
        std::fs::write(other.path().join("packages/old/stale.toml"), "stale").unwrap();
        let mut copy = Mirror::new(other.path().to_path_buf());
        copy.replace_with(&mirror.snapshot());
        assert!(!other.path().join("packages/old").exists());
        assert_eq!(std::fs::read_to_string(other.path().join("Game.nfproj")).unwrap(), "name = 1");
        let applied = copy.apply(&[
            FileChange { path: "packages/game/x.toml".into(), content: Some("x".into()) },
            FileChange { path: "../escape.toml".into(), content: Some("bad".into()) },
        ]);
        assert_eq!(applied, ["packages/game/x.toml"]);
        assert!(copy.apply(&[FileChange { path: "packages/game/x.toml".into(), content: Some("x".into()) }]).is_empty());
    }

    #[test]
    fn keeps_own_changes_until_the_host_confirms_them() {
        let mut in_flight = InFlight::default();
        let change = |content: &str| FileChange { path: "packages/a.toml".into(), content: Some(content.into()) };
        in_flight.sent(&[change("first")]);
        in_flight.sent(&[change("second")]);
        assert!(in_flight.is_pending("packages/a.toml"));
        assert!(!in_flight.accept_echo("packages/a.toml"));
        assert!(in_flight.accept_echo("packages/a.toml"));
        assert!(!in_flight.is_pending("packages/a.toml"));
    }
}
