mod clients;
mod describe;
mod edit;
mod server;

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use nf_project::{Id, Project};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

use crate::error::{CommandError, CommandResult};
use clients::{ClientKind, ClientSetup, InstallAction, InstallScope};

pub const DEFAULT_PORT: u16 = 7920;
const SETTINGS_FILE: &str = "mcp.toml";
const STATUS_EVENT: &str = "mcp";
const AGENT_FILES_EVENT: &str = "agent-files";
const AGENT_OPEN_EVENT: &str = "agent-open-graph";
const ACTIVITY_LIMIT: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct McpSettings {
    enabled: bool,
    port: u16,
}

impl Default for McpSettings {
    fn default() -> Self {
        Self { enabled: false, port: DEFAULT_PORT }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorContext {
    project_file: PathBuf,
    graph: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActivityEntry {
    id: u64,
    client: String,
    tool: String,
    summary: String,
    at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectedClient {
    name: String,
    last_seen: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    settings: McpSettings,
    running: bool,
    url: String,
    error: Option<String>,
    clients: Vec<ConnectedClient>,
    activity: Vec<ActivityEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentFilesMessage {
    project_root: PathBuf,
    paths: Vec<String>,
    agent: String,
    graph: Option<String>,
    nodes: Vec<String>,
    focus: Option<[f64; 2]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentOpenMessage {
    graph_id: String,
    node: Option<String>,
}

#[derive(Default)]
struct Inner {
    settings: McpSettings,
    running: Option<CancellationToken>,
    error: Option<String>,
    context: Option<EditorContext>,
    clients: BTreeMap<String, u64>,
    activity: VecDeque<ActivityEntry>,
    next_activity: u64,
}

#[derive(Default)]
pub struct McpState {
    inner: Mutex<Inner>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as u64).unwrap_or_default()
}

fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

fn settings_path(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(SETTINGS_FILE))
        .map_err(|error| CommandError::new("Settings folder unavailable", error.to_string()))
}

fn load_settings(app: &AppHandle) -> McpSettings {
    settings_path(app)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, settings: &McpSettings) -> CommandResult<()> {
    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    }
    let text = toml::to_string(settings).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    std::fs::write(&path, text).map_err(|error| CommandError::new("Settings not saved", error.to_string()))
}

impl McpState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn context(&self) -> Option<EditorContext> {
        self.lock().context.clone()
    }

    fn status(&self) -> McpStatus {
        let inner = self.lock();
        McpStatus {
            settings: inner.settings.clone(),
            running: inner.running.is_some(),
            url: url(inner.settings.port),
            error: inner.error.clone(),
            clients: inner
                .clients
                .iter()
                .map(|(name, last_seen)| ConnectedClient { name: name.clone(), last_seen: *last_seen })
                .collect(),
            activity: inner.activity.iter().cloned().collect(),
        }
    }

    fn publish(&self, app: &AppHandle) {
        let _ = app.emit(STATUS_EVENT, self.status());
    }

    pub fn record_activity(&self, app: &AppHandle, client: &str, tool: &str, summary: String) {
        {
            let mut inner = self.lock();
            let at = now();
            inner.clients.insert(client.to_owned(), at);
            inner.next_activity += 1;
            let entry = ActivityEntry {
                id: inner.next_activity,
                client: client.to_owned(),
                tool: tool.to_owned(),
                summary,
                at,
            };
            if inner.activity.len() == ACTIVITY_LIMIT {
                inner.activity.pop_front();
            }
            inner.activity.push_back(entry);
        }
        self.publish(app);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn announce_change(
        &self,
        app: &AppHandle,
        project: &Project,
        agent: &str,
        paths: Vec<PathBuf>,
        graph: Option<&Path>,
        nodes: &[Id],
        focus: Option<[f64; 2]>,
    ) {
        let root = project.root();
        let _ = app.emit(
            AGENT_FILES_EVENT,
            AgentFilesMessage {
                project_root: root.to_path_buf(),
                paths: paths.iter().map(|path| describe::relative(root, path)).collect(),
                agent: agent.to_owned(),
                graph: graph.map(|graph| describe::relative(root, graph)),
                nodes: nodes.iter().map(Id::to_string).collect(),
                focus,
            },
        );
    }

    pub fn open_graph(&self, app: &AppHandle, graph: Id, node: Option<Id>) {
        let _ = app.emit(
            AGENT_OPEN_EVENT,
            AgentOpenMessage {
                graph_id: graph.to_string(),
                node: node.map(|node| node.to_string()),
            },
        );
    }

    fn stop_server(&self) {
        if let Some(token) = self.lock().running.take() {
            token.cancel();
        }
    }

    fn apply(&self, app: &AppHandle, settings: McpSettings) {
        self.stop_server();
        let result = if settings.enabled { start_server(app, settings.port).map(Some) } else { Ok(None) };
        let mut inner = self.lock();
        inner.settings = settings;
        match result {
            Ok(token) => {
                inner.running = token;
                inner.error = None;
            }
            Err(message) => {
                inner.running = None;
                inner.error = Some(message);
            }
        }
    }

    pub fn shutdown(&self) {
        self.stop_server();
    }
}

fn start_server(app: &AppHandle, port: u16) -> Result<CancellationToken, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            format!("Port {port} is already in use. Pick another port.")
        } else {
            format!("The MCP server could not listen on port {port}: {error}")
        }
    })?;
    listener.set_nonblocking(true).map_err(|error| error.to_string())?;
    let token = CancellationToken::new();
    let handle = app.clone();
    let service = StreamableHttpService::new(
        move || Ok(server::NanosFlowServer::new(handle.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_json_response(true)
            .enforce_origin_validation()
            .with_cancellation_token(token.child_token()),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let shutdown = token.clone();
    let failure_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => axum::serve(listener, router)
                .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(message) = result {
            let state = failure_app.state::<McpState>();
            {
                let mut inner = state.lock();
                inner.running = None;
                inner.error = Some(message);
            }
            state.publish(&failure_app);
        }
    });
    Ok(token)
}

pub fn start(app: &AppHandle) {
    let state = app.state::<McpState>();
    state.apply(app, load_settings(app));
}

fn project_root(project_file: Option<&Path>) -> Option<PathBuf> {
    project_file.and_then(Path::parent).map(Path::to_path_buf)
}

#[tauri::command]
pub fn mcp_status(state: State<'_, McpState>) -> McpStatus {
    state.status()
}

#[tauri::command]
pub fn update_mcp_settings(app: AppHandle, state: State<'_, McpState>, settings: McpSettings) -> CommandResult<McpStatus> {
    if settings.port < 1024 {
        return Err(CommandError::new("Invalid port", "Pick a port between 1024 and 65535."));
    }
    save_settings(&app, &settings)?;
    state.apply(&app, settings);
    state.publish(&app);
    Ok(state.status())
}

#[tauri::command]
pub fn mcp_client_setups(state: State<'_, McpState>, scope: InstallScope, project_file: Option<PathBuf>) -> Vec<ClientSetup> {
    let port = state.lock().settings.port;
    clients::setups(&url(port), scope, project_root(project_file.as_deref()).as_deref())
}

#[tauri::command]
pub async fn install_mcp_client(
    app: AppHandle,
    state: State<'_, McpState>,
    kind: ClientKind,
    scope: InstallScope,
    project_file: Option<PathBuf>,
) -> CommandResult<String> {
    let settings = state.lock().settings.clone();
    if !settings.enabled || state.lock().running.is_none() {
        let enabled = McpSettings { enabled: true, ..settings };
        save_settings(&app, &enabled)?;
        state.apply(&app, enabled);
        state.publish(&app);
        if let Some(error) = state.lock().error.clone() {
            return Err(CommandError::new("The MCP server did not start", error));
        }
    }
    let port = state.lock().settings.port;
    let root = project_root(project_file.as_deref());
    match clients::install(kind, scope, &url(port), root.as_deref())? {
        InstallAction::Done(message) => Ok(message),
        InstallAction::OpenUrl(link, message) => {
            app.opener()
                .open_url(link, None::<&str>)
                .map_err(|error| CommandError::new("The app did not open", format!("{error}. Copy the configuration instead.")))?;
            Ok(message)
        }
    }
}

#[tauri::command]
pub fn report_editor_context(state: State<'_, McpState>, context: Option<EditorContext>) {
    state.lock().context = context;
}

#[cfg(test)]
mod tests;
