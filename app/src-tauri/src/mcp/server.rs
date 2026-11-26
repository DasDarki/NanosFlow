use std::path::PathBuf;

use nf_graph::GraphScope;
use nf_project::{EntryLocation, FunctionPin, GraphDocument, GraphKind, Id, Project, ScriptContext, VariableDocument};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, RequestMetaObject, ServerCapabilities, ServerConfig};
use rmcp::service::RoleServer;
use rmcp::{Peer, ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use super::McpState;
use super::describe::{self, GraphTarget};
use super::edit::{self, Operation};
use crate::error::CommandError;
use crate::graph_commands::{self, CatalogCache};
use crate::test_server::TestServerState;

const INSTRUCTIONS: &str = "NanosFlow is a visual scripting editor for the game nanos world. Graphs of nodes compile to Lua. \
You control the NanosFlow app the user has open right now and every change appears live in their editor. \
Start with get_project, read a graph with get_graph and find node types with search_nodes before editing. \
Exec pins define the order of execution (output exec pin to input exec pin), data pins carry values (output to input). \
Batch related changes into one edit_graph call: the user can undo each call in one step. \
After editing, check the diagnostics in the result, and use build_project or play_test_server to try the result.";

const DEFAULT_SEARCH_LIMIT: usize = 25;
const DEFAULT_LOG_LINES: usize = 80;

#[derive(Clone)]
pub struct NanosFlowServer {
    app: AppHandle,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GraphRequest {
    #[schemars(description = "Graph path from get_project, or the graph id. Left out, the graph open in the editor is used.")]
    graph: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    #[schemars(description = "Words to look for in node titles, categories and descriptions, for example 'spawn character' or 'print'.")]
    query: String,
    #[schemars(description = "Graph whose scope to search, so its variables, functions and context apply. Left out, the open graph is used.")]
    graph: Option<String>,
    #[schemars(description = "Maximum number of results, 25 by default.")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NodeTypeRequest {
    #[serde(rename = "type")]
    #[schemars(description = "Node type id from search_nodes or get_graph.")]
    node_type: String,
    #[schemars(description = "Graph whose scope to use. Left out, the open graph is used.")]
    graph: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditRequest {
    #[schemars(description = "Graph path from get_project, or the graph id. Left out, the graph open in the editor is used.")]
    graph: Option<String>,
    #[schemars(description = "Operations applied in order. If one fails, nothing is changed.")]
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OpenRequest {
    #[schemars(description = "Graph path from get_project, or the graph id.")]
    graph: String,
    #[schemars(description = "Node id to center the view on.")]
    node: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum NewGraphKind {
    EventGraph,
    Function,
    Macro,
    Class,
    Struct,
    Enum,
    Event,
    Interface,
}

impl From<NewGraphKind> for GraphKind {
    fn from(kind: NewGraphKind) -> GraphKind {
        match kind {
            NewGraphKind::EventGraph => GraphKind::EventGraph,
            NewGraphKind::Function => GraphKind::Function,
            NewGraphKind::Macro => GraphKind::Macro,
            NewGraphKind::Class => GraphKind::Class,
            NewGraphKind::Struct => GraphKind::Struct,
            NewGraphKind::Enum => GraphKind::Enum,
            NewGraphKind::Event => GraphKind::Event,
            NewGraphKind::Interface => GraphKind::Interface,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GraphContext {
    Server,
    Client,
    Shared,
}

impl From<GraphContext> for ScriptContext {
    fn from(context: GraphContext) -> ScriptContext {
        match context {
            GraphContext::Server => ScriptContext::Server,
            GraphContext::Client => ScriptContext::Client,
            GraphContext::Shared => ScriptContext::Shared,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateGraphRequest {
    #[schemars(description = "Package folder from get_project.")]
    package: String,
    #[schemars(description = "Where the script runs: server, client or shared.")]
    context: GraphContext,
    #[schemars(description = "Name of the new graph, for example 'SpawnPlayers'.")]
    name: String,
    kind: NewGraphKind,
    #[schemars(description = "Folders inside the context to create the graph in.")]
    #[serde(default)]
    folders: Vec<String>,
    #[schemars(description = "For classes: the class to inherit from, for example 'Character' or 'Prop'.")]
    parent_class: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VariableEntry {
    name: String,
    #[serde(rename = "type")]
    #[schemars(description = "Type name, for example 'number', 'string', 'boolean', 'Vector' or a class like 'Character'.")]
    type_name: String,
    #[schemars(description = "Default value, written like a literal pin value.")]
    default: Option<Value>,
    description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VariablesRequest {
    #[schemars(description = "Graph path or id. Left out, the open graph is used.")]
    graph: Option<String>,
    #[schemars(description = "Variables to add, or to update when the name already exists.")]
    #[serde(default)]
    set: Vec<VariableEntry>,
    #[schemars(description = "Names of variables to remove.")]
    #[serde(default)]
    remove: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SignaturePin {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SignatureRequest {
    #[schemars(description = "Function, macro or event graph. Left out, the open graph is used.")]
    graph: Option<String>,
    parameters: Vec<SignaturePin>,
    #[schemars(description = "Return values. Events have none.")]
    #[serde(default)]
    returns: Vec<SignaturePin>,
    description: Option<String>,
    #[schemars(description = "Pure functions have no exec pins.")]
    pure: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlayRequest {
    #[schemars(description = "Restart the server instead of reloading the packages.")]
    #[serde(default)]
    restart: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LogRequest {
    #[schemars(description = "Number of recent console lines, 80 by default.")]
    lines: Option<usize>,
}

fn command_error(error: CommandError) -> String {
    let mut message = format!("{}: {}", error.title, error.message);
    for issue in &error.issues {
        if !error.message.contains(&issue.message) {
            message.push_str(&format!(" {}", issue.message));
        }
    }
    message
}

fn to_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn client_name(peer: &Peer<RoleServer>, meta: &RequestMetaObject) -> String {
    let info: Option<Implementation> = meta.client_info().or_else(|| peer.peer_info().map(|info| info.client_info.clone()));
    info.map(|info| info.title.filter(|title| !title.trim().is_empty()).unwrap_or(info.name))
        .map(|name| super::clients::display_name(&name))
        .unwrap_or_else(|| "AI agent".to_owned())
}

fn pins_with_ids(existing: &[FunctionPin], wanted: Vec<SignaturePin>) -> Vec<FunctionPin> {
    wanted
        .into_iter()
        .map(|pin| {
            let id = existing
                .iter()
                .find(|current| current.name == pin.name)
                .map(|current| current.id.clone())
                .unwrap_or_else(|| Id::generate().to_string().chars().rev().take(8).collect::<String>().chars().rev().collect());
            FunctionPin { id, name: pin.name, type_name: pin.type_name }
        })
        .collect()
}

fn json_to_toml(value: Value, what: &str) -> Result<toml::Value, String> {
    if value.is_null() {
        return Err(format!("{what} cannot be null."));
    }
    toml::Value::try_from(value).map_err(|error| format!("{what} is not a valid value: {error}"))
}

impl NanosFlowServer {
    pub fn new(app: AppHandle) -> Self {
        let mut tool_router = Self::tool_router();
        for tool in app.state::<McpState>().permissions().blocked_tools() {
            tool_router.remove_route(tool);
        }
        Self { app, tool_router }
    }

    fn state(&self) -> tauri::State<'_, McpState> {
        self.app.state::<McpState>()
    }

    fn project(&self) -> Result<(Project, Option<PathBuf>), String> {
        let context = self
            .state()
            .context()
            .ok_or_else(|| "No project is open in NanosFlow. Ask the user to open one, then try again.".to_owned())?;
        let project = Project::open(&context.project_file).map_err(|error| command_error(error.into()))?;
        Ok((project, context.graph))
    }

    fn target(&self, graph: Option<&str>) -> Result<(Project, GraphTarget), String> {
        let (project, open_graph) = self.project()?;
        let target = describe::find_graph(&project, graph, open_graph.as_deref())?;
        Ok((project, target))
    }

    fn catalog(&self, project: &Project) -> Result<std::sync::Arc<graph_commands::CachedCatalog>, String> {
        self.app
            .state::<CatalogCache>()
            .get(project.manifest().api_channel)
            .map_err(command_error)
    }

    fn scope_data(&self, project: &Project, target: &GraphTarget) -> Result<(std::sync::Arc<graph_commands::CachedCatalog>, nf_graph::ScopeData), String> {
        let cached = self.catalog(project)?;
        let scope = nf_graph::build_scope(&cached.catalog, &target.package.path, &target.graph.path).map_err(|error| command_error(error.into()))?;
        Ok((cached, scope))
    }

    fn record(&self, peer: &Peer<RoleServer>, meta: &RequestMetaObject, tool: &str, summary: impl Into<String>) -> String {
        let name = client_name(peer, meta);
        self.state().record_activity(&self.app, &name, tool, summary.into());
        name
    }
}

#[tool_router]
impl NanosFlowServer {
    #[tool(description = "Show the open project: its packages and every graph with path, kind, script context and node count, plus the graph open in the editor.")]
    async fn get_project(&self, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, open_graph) = self.project()?;
        self.record(&peer, &meta, "get_project", "Read the project");
        Ok(to_text(&describe::project_overview(&project, open_graph.as_deref())))
    }

    #[tool(description = "Read a graph: every node with its type, position, input pins (with values or the pin they are connected from) and output pins (with the pins they connect to), the graph signature, variables and current diagnostics. Links are written as '<node id>.<pin>'.")]
    async fn get_graph(&self, Parameters(request): Parameters<GraphRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, target) = self.target(request.graph.as_deref())?;
        let document = GraphDocument::load(&target.graph.path).map_err(|error| command_error(error.into()))?;
        let (cached, scope_data) = self.scope_data(&project, &target)?;
        let scope = GraphScope::new(&cached.catalog, scope_data);
        let diagnostics = nf_graph::analyze_graph(&scope, target.graph.context, target.graph.kind, &document.nodes);
        self.record(&peer, &meta, "get_graph", format!("Read {}", target.graph.name));
        Ok(to_text(&describe::graph_view(&project, &target, &document, &scope, &diagnostics)))
    }

    #[tool(description = "Search the node types available in a graph: nanos world API functions and events, flow control, math, and the project's own variables, functions, events and classes. Returns type ids to use with edit_graph.")]
    async fn search_nodes(&self, Parameters(request): Parameters<SearchRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, target) = self.target(request.graph.as_deref())?;
        let (cached, scope_data) = self.scope_data(&project, &target)?;
        let scope = GraphScope::new(&cached.catalog, scope_data);
        self.record(&peer, &meta, "search_nodes", format!("Searched for '{}'", request.query));
        Ok(to_text(&describe::search(
            &scope,
            Some(target.graph.context),
            &request.query,
            request.limit.unwrap_or(DEFAULT_SEARCH_LIMIT).clamp(1, 100),
        )))
    }

    #[tool(description = "Describe one node type in detail: every input and output pin with type, default and description.")]
    async fn describe_node_type(&self, Parameters(request): Parameters<NodeTypeRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, target) = self.target(request.graph.as_deref())?;
        let (cached, scope_data) = self.scope_data(&project, &target)?;
        let scope = GraphScope::new(&cached.catalog, scope_data);
        let definition = scope
            .get(&request.node_type)
            .ok_or_else(|| format!("There is no node type '{}' in this graph. Use search_nodes to find it.", request.node_type))?;
        self.record(&peer, &meta, "describe_node_type", format!("Looked up {}", definition.title));
        Ok(to_text(&describe::node_type_view(definition)))
    }

    #[tool(description = "Change a graph with a batch of operations: add, remove, move, connect, disconnect, set_value and set_property. The change shows up live in the editor and the user can undo the whole call in one step. Returns the ids of added nodes by their ref and the diagnostics after the change.")]
    async fn edit_graph(&self, Parameters(request): Parameters<EditRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        if request.operations.is_empty() {
            return Err("Pass at least one operation.".to_owned());
        }
        let (project, target) = self.target(request.graph.as_deref())?;
        let document = GraphDocument::load(&target.graph.path).map_err(|error| command_error(error.into()))?;
        let (cached, scope_data) = self.scope_data(&project, &target)?;
        let scope = GraphScope::new(&cached.catalog, scope_data);
        let count = request.operations.len();
        let outcome = edit::apply_operations(&scope, document.nodes, request.operations)?;
        nf_project::apply_graph_changes(&target.graph.path, &outcome.upserts, &outcome.deletions).map_err(|error| command_error(error.into()))?;

        let after = GraphDocument::load(&target.graph.path).map_err(|error| command_error(error.into()))?;
        let diagnostics = nf_graph::analyze_graph(&scope, target.graph.context, target.graph.kind, &after.nodes);
        let name = self.record(
            &peer,
            &meta,
            "edit_graph",
            format!("Changed {} with {count} {}", target.graph.name, if count == 1 { "operation" } else { "operations" }),
        );
        let paths = describe::changed_paths(&target.graph.path, &outcome.upserts, &outcome.deletions);
        let focus = outcome
            .touched
            .iter()
            .find_map(|id| after.nodes.iter().find(|node| node.id == *id))
            .map(|node| node.position);
        self.state().announce_change(&self.app, &project, &name, paths, Some(&target.graph.path), &outcome.touched, focus);

        Ok(to_text(&json!({
            "graph": describe::relative(project.root(), &target.graph.path),
            "created": outcome.created,
            "changedNodes": outcome.upserts.len(),
            "removedNodes": outcome.deletions.len(),
            "diagnostics": diagnostics,
        })))
    }

    #[tool(description = "Open a graph in the user's editor, optionally centered on a node, to show them something.")]
    async fn open_graph(&self, Parameters(request): Parameters<OpenRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (_, target) = self.target(Some(&request.graph))?;
        let node = request.node.map(|node| node.parse::<Id>().map_err(|_| format!("'{node}' is not a node id."))).transpose()?;
        self.state().open_graph(&self.app, target.graph.id, node);
        self.record(&peer, &meta, "open_graph", format!("Opened {}", target.graph.name));
        Ok(format!("Opened {} in the editor.", target.graph.name))
    }

    #[tool(description = "Create a graph in a package: an event graph, function, macro, class, struct, enum, event or interface.")]
    async fn create_graph(&self, Parameters(request): Parameters<CreateGraphRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, _) = self.project()?;
        let package = project
            .packages()
            .iter()
            .find(|package| package.folder.eq_ignore_ascii_case(&request.package))
            .ok_or_else(|| format!("There is no package '{}'. Use get_project to list the packages.", request.package))?
            .clone();
        let issues = nf_project::validate_entry_name(&request.name);
        if !issues.is_empty() {
            return Err(issues.into_iter().map(|issue| issue.message).collect::<Vec<_>>().join(" "));
        }
        let location = EntryLocation {
            package_path: package.path.clone(),
            context: request.context.into(),
            folders: request.folders,
        };
        let directory = location.directory().join(&request.name);
        let kind = GraphKind::from(request.kind);
        let view = graph_commands::create_graph(
            self.app.state::<CatalogCache>(),
            project.manifest().api_channel,
            project.project_file().to_path_buf(),
            location,
            request.name.clone(),
            kind,
            request.parent_class,
        )
        .await
        .map_err(command_error)?;
        let created = view
            .packages
            .iter()
            .flat_map(|package| &package.graphs)
            .find(|graph| graph.path == directory)
            .map(|graph| describe::relative(project.root(), &graph.path))
            .unwrap_or_else(|| describe::relative(project.root(), &directory));
        let name = self.record(&peer, &meta, "create_graph", format!("Created {}", request.name));
        self.state().announce_change(&self.app, &project, &name, vec![directory], None, &[], None);
        Ok(to_text(&json!({ "created": created })))
    }

    #[tool(description = "Add, update or remove variables of a graph. Variables of a class or event graph can be read and set with nodes from search_nodes.")]
    async fn set_variables(&self, Parameters(request): Parameters<VariablesRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, target) = self.target(request.graph.as_deref())?;
        let current = nf_project::load_variables(&target.graph.path).map_err(|error| command_error(error.into()))?;
        let mut upserts: Vec<VariableDocument> = Vec::new();
        for entry in request.set {
            let mut variable = current.iter().find(|variable| variable.name == entry.name).cloned().unwrap_or_else(|| VariableDocument {
                id: Id::generate(),
                name: entry.name.clone(),
                type_name: entry.type_name.clone(),
                default: None,
                description: String::new(),
                sync: false,
            });
            variable.type_name = entry.type_name;
            if let Some(default) = entry.default {
                variable.default = Some(json_to_toml(default, &format!("The default of '{}'", entry.name))?);
            }
            if let Some(description) = entry.description {
                variable.description = description;
            }
            upserts.push(variable);
        }
        let mut deletions = Vec::new();
        for name in &request.remove {
            let variable = current
                .iter()
                .find(|variable| variable.name == *name)
                .ok_or_else(|| format!("The graph has no variable '{name}'."))?;
            deletions.push(variable.id);
        }
        let paths: Vec<PathBuf> = upserts
            .iter()
            .map(|variable| variable.id)
            .chain(deletions.iter().copied())
            .map(|id| VariableDocument::path_for(&target.graph.path, id))
            .collect();
        graph_commands::save_variables(
            self.app.state::<CatalogCache>(),
            project.manifest().api_channel,
            target.package.path.clone(),
            target.graph.path.clone(),
            upserts,
            deletions,
        )
        .await
        .map_err(command_error)?;
        let name = self.record(&peer, &meta, "set_variables", format!("Changed the variables of {}", target.graph.name));
        self.state().announce_change(&self.app, &project, &name, paths, Some(&target.graph.path), &[], None);
        let variables = nf_project::load_variables(&target.graph.path).map_err(|error| command_error(error.into()))?;
        Ok(to_text(&json!({
            "variables": variables.iter().map(|variable| json!({"name": variable.name, "type": variable.type_name})).collect::<Vec<_>>()
        })))
    }

    #[tool(description = "Set the parameters and return values of a function, macro or event graph. Existing pins keep their links when their name stays the same.")]
    async fn set_signature(&self, Parameters(request): Parameters<SignatureRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, target) = self.target(request.graph.as_deref())?;
        let manifest = nf_project::GraphManifest::read(&target.graph.path).map_err(|error| command_error(error.into()))?;
        let mut signature = manifest.signature.clone();
        signature.parameters = pins_with_ids(&manifest.signature.parameters, request.parameters);
        signature.returns = pins_with_ids(&manifest.signature.returns, request.returns);
        if let Some(description) = request.description {
            signature.description = description;
        }
        if let Some(pure) = request.pure {
            signature.pure = pure;
        }
        let channel = project.manifest().api_channel;
        match manifest.kind {
            GraphKind::Function | GraphKind::Macro => {
                graph_commands::update_function_signature(
                    self.app.state::<CatalogCache>(),
                    channel,
                    target.package.path.clone(),
                    target.graph.path.clone(),
                    signature,
                )
                .await
                .map_err(command_error)?;
            }
            GraphKind::Event => {
                graph_commands::update_event(
                    self.app.state::<CatalogCache>(),
                    channel,
                    target.package.path.clone(),
                    target.graph.path.clone(),
                    manifest.direction.unwrap_or_default(),
                    signature,
                )
                .await
                .map_err(command_error)?;
            }
            _ => return Err(format!("{} is not a function, macro or event, so it has no signature.", target.graph.name)),
        }
        let name = self.record(&peer, &meta, "set_signature", format!("Changed the signature of {}", target.graph.name));
        self.state().announce_change(
            &self.app,
            &project,
            &name,
            vec![target.graph.path.join(nf_project::GraphManifest::FILE_NAME)],
            Some(&target.graph.path),
            &[],
            None,
        );
        Ok(format!("The signature of {} was saved.", target.graph.name))
    }

    #[tool(description = "Compile the whole project to Lua and report problems per package.")]
    async fn build_project(&self, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, _) = self.project()?;
        let cached = self.catalog(&project)?;
        let report = nf_compiler::build_project(&project, &cached.catalog).map_err(|error| command_error(error.into()))?;
        self.record(
            &peer,
            &meta,
            "build_project",
            format!("Built the project: {} errors, {} warnings", report.error_count, report.warning_count),
        );
        Ok(to_text(&serde_json::to_value(&report).unwrap_or(Value::Null)))
    }

    #[tool(description = "Show the Lua code a graph compiles to, with its problems.")]
    async fn preview_lua(&self, Parameters(request): Parameters<GraphRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, target) = self.target(request.graph.as_deref())?;
        let preview = graph_commands::preview_lua(
            self.app.state::<CatalogCache>(),
            project.manifest().api_channel,
            target.package.path.clone(),
            target.graph.id,
        )
        .await
        .map_err(command_error)?;
        self.record(&peer, &meta, "preview_lua", format!("Read the Lua of {}", target.graph.name));
        Ok(to_text(&serde_json::to_value(&preview).unwrap_or(Value::Null)))
    }

    #[tool(description = "Build the project and start the local nanos world test server, or reload the packages when it already runs. Needs an installed test server.")]
    async fn play_test_server(&self, Parameters(request): Parameters<PlayRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let (project, _) = self.project()?;
        let result = crate::test_server::play_test_server(
            self.app.clone(),
            self.app.state::<TestServerState>(),
            self.app.state::<CatalogCache>(),
            project.manifest().api_channel,
            project.project_file().to_path_buf(),
            request.restart,
        )
        .await
        .map_err(command_error)?;
        self.record(&peer, &meta, "play_test_server", "Started the test server");
        Ok(to_text(&serde_json::to_value(&result).unwrap_or(Value::Null)))
    }

    #[tool(description = "Stop the local test server.")]
    async fn stop_test_server(&self, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let stopped = crate::test_server::stop_test_server(self.app.clone(), self.app.state::<TestServerState>())
            .await
            .map_err(command_error)?;
        self.record(&peer, &meta, "stop_test_server", "Stopped the test server");
        Ok(if stopped { "The test server was stopped." } else { "The test server was not running." }.to_owned())
    }

    #[tool(description = "Read the recent console output of the test server, including Lua errors and prints.")]
    async fn read_server_log(&self, Parameters(request): Parameters<LogRequest>, peer: Peer<RoleServer>, meta: RequestMetaObject) -> Result<String, String> {
        let state = self.app.state::<TestServerState>();
        let lines = state.recent_output(request.lines.unwrap_or(DEFAULT_LOG_LINES).clamp(1, 500));
        self.record(&peer, &meta, "read_server_log", "Read the server console");
        Ok(to_text(&json!({ "running": state.is_running(), "lines": lines })))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for NanosFlowServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("nanosflow", env!("CARGO_PKG_VERSION")).with_title("NanosFlow"))
            .with_instructions(INSTRUCTIONS)
    }
}
