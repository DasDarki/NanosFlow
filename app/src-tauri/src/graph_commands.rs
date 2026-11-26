use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nf_api::{ApiChannel, ApiDatabase};
use nf_graph::{Diagnostic, GraphScope, NodeCatalog, ScopeData};
use nf_project::{
    EntryLocation, FunctionSignature, GraphDocument, GraphKind, Id, NodeDocument, Project, ProjectError, ScriptContext,
    VariableDocument,
};
use tauri::State;
use tauri::ipc::Response;

use crate::commands::ProjectView;
use crate::error::{CommandError, CommandResult};

pub(crate) struct CachedCatalog {
    pub(crate) catalog: NodeCatalog,
    json: String,
}

#[derive(Default)]
pub struct CatalogCache {
    entries: Mutex<HashMap<ApiChannel, Arc<CachedCatalog>>>,
}

impl CatalogCache {
    pub(crate) fn get(&self, channel: ApiChannel) -> CommandResult<Arc<CachedCatalog>> {
        if let Some(cached) = self.entries.lock().ok().and_then(|entries| entries.get(&channel).cloned()) {
            return Ok(cached);
        }
        let database = ApiDatabase::bundled(channel).map_err(|error| {
            CommandError::new(
                "API data unavailable",
                format!("The {} API could not be loaded: {error}", channel.display_name()),
            )
        })?;
        let catalog = NodeCatalog::from_database(&database);
        let json = serde_json::to_string(&catalog.payload())
            .map_err(|error| CommandError::new("API data unavailable", error.to_string()))?;
        let cached = Arc::new(CachedCatalog { catalog, json });
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(channel, cached.clone());
        }
        Ok(cached)
    }
}

#[tauri::command]
pub async fn node_catalog(cache: State<'_, CatalogCache>, channel: ApiChannel) -> CommandResult<Response> {
    Ok(Response::new(cache.get(channel)?.json.clone()))
}

fn titled(error: ProjectError, title: &str) -> CommandError {
    let mut command_error = CommandError::from(error);
    command_error.title = title.to_owned();
    command_error
}

fn ensure_inside_project(project_file: &Path, path: &Path) -> CommandResult<()> {
    let packages = project_file
        .parent()
        .map(|root| root.join(Project::PACKAGES_DIRECTORY))
        .and_then(|packages| packages.canonicalize().ok());
    let target = path.canonicalize().ok();
    match (packages, target) {
        (Some(packages), Some(target)) if target.starts_with(&packages) && target != packages => Ok(()),
        _ => Err(CommandError::new(
            "Outside of the project",
            format!("'{}' is not part of this project's packages.", path.display()),
        )),
    }
}

fn reopen(project_file: &Path) -> CommandResult<ProjectView> {
    let project = Project::open(project_file)?;
    Ok(ProjectView::from(&project))
}

fn build_scope(catalog: &NodeCatalog, package_path: &Path, graph_path: &Path) -> CommandResult<ScopeData> {
    nf_graph::build_scope(catalog, package_path, graph_path)
        .map_err(|error| titled(error, "The package could not be read"))
}

fn issues_error(title: &str, issues: Vec<nf_project::FieldIssue>) -> CommandError {
    let mut error = CommandError::new(
        title,
        issues.iter().map(|issue| issue.message.as_str()).collect::<Vec<_>>().join(" "),
    );
    error.issues = issues;
    error
}

#[tauri::command]
pub async fn load_graph(path: PathBuf) -> CommandResult<GraphDocument> {
    Ok(GraphDocument::load(&path)?)
}

#[tauri::command]
pub async fn save_graph_changes(path: PathBuf, upserts: Vec<NodeDocument>, deletions: Vec<Id>) -> CommandResult<()> {
    nf_project::apply_graph_changes(&path, &upserts, &deletions)
        .map_err(|error| titled(error, "Changes could not be saved"))
}

#[tauri::command]
pub async fn graph_scope(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_path: PathBuf,
) -> CommandResult<ScopeData> {
    let cached = cache.get(channel)?;
    build_scope(&cached.catalog, &package_path, &graph_path)
}

#[tauri::command]
pub async fn analyze_graph(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_path: PathBuf,
    context: ScriptContext,
    kind: GraphKind,
    nodes: Vec<NodeDocument>,
) -> CommandResult<Vec<Diagnostic>> {
    let cached = cache.get(channel)?;
    let scope = GraphScope::new(&cached.catalog, build_scope(&cached.catalog, &package_path, &graph_path)?);
    Ok(nf_graph::analyze_graph(&scope, context, kind, &nodes))
}

#[tauri::command]
pub async fn update_function_signature(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_path: PathBuf,
    signature: FunctionSignature,
) -> CommandResult<ScopeData> {
    let cached = cache.get(channel)?;
    let scope = GraphScope::new(&cached.catalog, build_scope(&cached.catalog, &package_path, &graph_path)?);
    let is_macro = nf_project::GraphManifest::read(&graph_path).is_ok_and(|manifest| manifest.kind == GraphKind::Macro);
    let issues = if is_macro {
        nf_graph::validate_macro_signature(&signature, scope.types())
    } else {
        nf_graph::validate_signature(&signature, scope.types())
    };
    if !issues.is_empty() {
        return Err(issues_error(if is_macro { "The macro pins need attention" } else { "The function signature needs attention" }, issues));
    }
    nf_project::update_function_signature(&graph_path, signature)
        .map_err(|error| titled(error, "The function could not be saved"))?;
    build_scope(&cached.catalog, &package_path, &graph_path)
}

#[tauri::command]
pub async fn update_event(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_path: PathBuf,
    direction: nf_project::EventDirection,
    signature: FunctionSignature,
) -> CommandResult<nf_project::GraphManifest> {
    let cached = cache.get(channel)?;
    let scope = GraphScope::new(&cached.catalog, build_scope(&cached.catalog, &package_path, &graph_path)?);
    let issues = nf_graph::validate_signature(&signature, scope.types());
    if !issues.is_empty() {
        return Err(issues_error("The event needs attention", issues));
    }
    nf_project::update_event(&graph_path, direction, signature).map_err(|error| titled(error, "The event could not be saved"))
}

#[tauri::command]
pub async fn save_variables(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_path: PathBuf,
    variables: Vec<VariableDocument>,
    deletions: Vec<Id>,
) -> CommandResult<ScopeData> {
    let cached = cache.get(channel)?;
    let scope = GraphScope::new(&cached.catalog, build_scope(&cached.catalog, &package_path, &graph_path)?);
    let issues = nf_graph::validate_variables(&variables, scope.types());
    if !issues.is_empty() {
        return Err(issues_error("The variables need attention", issues));
    }
    nf_project::apply_variable_changes(&graph_path, &variables, &deletions)
        .map_err(|error| titled(error, "The variables could not be saved"))?;
    build_scope(&cached.catalog, &package_path, &graph_path)
}

#[tauri::command]
pub fn validate_entry_name(name: String) -> Vec<nf_project::FieldIssue> {
    nf_project::validate_entry_name(&name)
}

fn type_name_issues(catalog: &NodeCatalog, package_path: &Path, name: &str) -> CommandResult<Vec<nf_project::FieldIssue>> {
    let mut issues = nf_project::validate_class_name(name);
    if !issues.is_empty() {
        return Ok(issues);
    }
    let tree = nf_project::scan_package_tree(package_path).map_err(|error| titled(error, "The package could not be read"))?;
    if catalog.types().is_known(name) {
        issues.push(nf_project::FieldIssue::error(
            "name",
            format!("'{name}' is already a type of the nanos world API. Pick another name, for example 'My{name}'."),
        ));
    } else if let Some(existing) = tree.graphs.iter().find(|graph| {
        matches!(graph.kind, GraphKind::Class | GraphKind::Struct | GraphKind::Enum | GraphKind::Interface) && graph.name.eq_ignore_ascii_case(name)
    }) {
        let what = match existing.kind {
            GraphKind::Class => "class",
            GraphKind::Enum => "enum",
            GraphKind::Interface => "interface",
            _ => "struct",
        };
        issues.push(nf_project::FieldIssue::error(
            "name",
            format!("This package already has a {what} called '{}'. Type names must be unique in a package.", existing.name),
        ));
    }
    Ok(issues)
}

fn validate_new_class(catalog: &NodeCatalog, package_path: &Path, name: &str, parent: &str) -> CommandResult<()> {
    let mut issues = type_name_issues(catalog, package_path, name)?;
    let project_classes = nf_graph::project_classes(catalog, package_path)
        .map_err(|error| titled(error, "The package could not be read"))?;
    let parent_is_valid = catalog.is_inheritable(parent)
        || project_classes.iter().any(|class| class.name == parent);
    if !parent_is_valid {
        issues.push(nf_project::FieldIssue::error(
            "parent",
            format!("'{parent}' cannot be inherited from. Pick an entity class like Character, Prop or Weapon, or one of your classes."),
        ));
    }
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues_error("The class settings need attention", issues))
    }
}

#[tauri::command]
pub async fn update_package_requirements(project_file: PathBuf, package_path: PathBuf, requirements: Vec<String>) -> CommandResult<ProjectView> {
    let project = Project::open(&project_file)?;
    project.update_requirements(&package_path, &requirements).map_err(|error| match error {
        nf_project::ProjectError::InvalidOptions { issues } => issues_error("The requirements need attention", issues),
        error => titled(error, "The requirements could not be saved"),
    })?;
    reopen(&project_file)
}

#[tauri::command]
pub async fn update_custom_settings(project_file: PathBuf, package_path: PathBuf, settings: Vec<nf_project::CustomSetting>) -> CommandResult<ProjectView> {
    let project = Project::open(&project_file)?;
    project.update_custom_settings(&package_path, settings).map_err(|error| match error {
        nf_project::ProjectError::InvalidOptions { issues } => issues_error("The custom settings need attention", issues),
        error => titled(error, "The custom settings could not be saved"),
    })?;
    reopen(&project_file)
}

#[tauri::command]
pub async fn create_graph(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    project_file: PathBuf,
    location: EntryLocation,
    name: String,
    kind: GraphKind,
    parent: Option<String>,
) -> CommandResult<ProjectView> {
    ensure_inside_project(&project_file, &location.package_path)?;
    if kind == GraphKind::Class {
        let cached = cache.get(channel)?;
        let parent = parent.unwrap_or_default();
        validate_new_class(&cached.catalog, &location.package_path, &name, &parent)?;
        nf_project::create_class(&location.directory(), &name, &parent)
            .map_err(|error| titled(error, "The class could not be created"))?;
    } else {
        if matches!(kind, GraphKind::Struct | GraphKind::Enum | GraphKind::Interface) {
            let cached = cache.get(channel)?;
            let issues = type_name_issues(&cached.catalog, &location.package_path, &name)?;
            if !issues.is_empty() {
                return Err(issues_error("The struct name needs attention", issues));
            }
        }
        nf_project::create_graph(&location.directory(), &name, kind)
            .map_err(|error| titled(error, "The graph could not be created"))?;
    }
    reopen(&project_file)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatedClass {
    project: ProjectView,
    scope: ScopeData,
}

#[derive(serde::Deserialize)]
pub struct ClassUpdate {
    parent: String,
    description: String,
    interfaces: Vec<String>,
    exported: bool,
}

#[tauri::command]
pub async fn update_class(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    project_file: PathBuf,
    package_path: PathBuf,
    graph_path: PathBuf,
    settings: ClassUpdate,
) -> CommandResult<UpdatedClass> {
    let ClassUpdate { parent, description, interfaces, exported } = settings;
    ensure_inside_project(&project_file, &graph_path)?;
    let cached = cache.get(channel)?;
    let class_name = graph_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let classes = nf_graph::project_classes(&cached.catalog, &package_path)
        .map_err(|error| titled(error, "The package could not be read"))?;
    let is_project_class = classes.iter().any(|class| class.name == parent);
    let issue = if parent == class_name {
        Some(format!("{class_name} cannot inherit from itself."))
    } else if classes
        .iter()
        .any(|class| class.name == parent && class.parents.contains(&class_name))
    {
        Some(format!(
            "{parent} already inherits from {class_name}, so {class_name} cannot inherit from {parent}. That would be a loop."
        ))
    } else if !is_project_class && !cached.catalog.is_inheritable(&parent) {
        Some(format!("'{parent}' cannot be inherited from. Pick an entity class like Character, Prop or Weapon, or one of your classes."))
    } else {
        None
    };
    if let Some(message) = issue {
        return Err(issues_error(
            "The class settings need attention",
            vec![nf_project::FieldIssue::error("parent", message)],
        ));
    }
    let tree = nf_project::scan_package_tree(&package_path).map_err(|error| titled(error, "The package could not be read"))?;
    let mut implemented = Vec::new();
    for name in &interfaces {
        let Some(interface) = tree.graphs.iter().find(|graph| graph.kind == GraphKind::Interface && graph.name == *name) else {
            return Err(issues_error(
                "The class settings need attention",
                vec![nf_project::FieldIssue::error("interfaces", format!("There is no interface called '{name}' in this package."))],
            ));
        };
        if !implemented.contains(name) {
            implemented.push(name.clone());
        }
        let functions = nf_project::GraphManifest::read(&interface.path)
            .map_err(|error| titled(error, "The interface could not be read"))?
            .functions;
        nf_project::add_interface_methods(&graph_path, &functions).map_err(|error| titled(error, "The interface methods could not be added"))?;
    }
    nf_project::update_class(
        &graph_path,
        nf_project::ClassSettings {
            parent: &parent,
            description: &description,
            interfaces: &implemented,
            exported,
        },
    )
        .map_err(|error| titled(error, "The class could not be saved"))?;
    Ok(UpdatedClass {
        project: reopen(&project_file)?,
        scope: build_scope(&cached.catalog, &package_path, &graph_path)?,
    })
}

#[tauri::command]
pub async fn update_interface(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_path: PathBuf,
    description: String,
    functions: Vec<nf_project::InterfaceFunction>,
) -> CommandResult<nf_project::GraphManifest> {
    let cached = cache.get(channel)?;
    let scope = GraphScope::new(&cached.catalog, build_scope(&cached.catalog, &package_path, &graph_path)?);
    let issues = nf_graph::validate_interface(&functions, scope.types());
    if !issues.is_empty() {
        return Err(issues_error("The interface needs attention", issues));
    }
    nf_project::update_interface(&graph_path, &description, functions).map_err(|error| titled(error, "The interface could not be saved"))
}

#[tauri::command]
pub async fn graph_conflicts(project_file: PathBuf, graph_path: PathBuf) -> CommandResult<Vec<nf_project::FileConflict>> {
    ensure_inside_project(&project_file, &graph_path)?;
    nf_project::graph_conflicts(&graph_path).map_err(|error| titled(error, "The conflicts could not be read"))
}

#[tauri::command]
pub async fn resolve_conflict(
    project_file: PathBuf,
    graph_path: PathBuf,
    file: String,
    choice: nf_project::ConflictChoice,
) -> CommandResult<Vec<nf_project::FileConflict>> {
    ensure_inside_project(&project_file, &graph_path)?;
    nf_project::resolve_conflict(&graph_path, &file, choice).map_err(|error| titled(error, "The conflict could not be resolved"))?;
    nf_project::graph_conflicts(&graph_path).map_err(|error| titled(error, "The conflicts could not be read"))
}

#[tauri::command]
pub async fn committed_graph(project_file: PathBuf, graph_path: PathBuf) -> CommandResult<Option<nf_project::CommittedGraph>> {
    ensure_inside_project(&project_file, &graph_path)?;
    Ok(nf_project::committed_graph(&graph_path))
}

#[tauri::command]
pub async fn create_dispatcher(project_file: PathBuf, class_path: PathBuf, name: String) -> CommandResult<ProjectView> {
    ensure_inside_project(&project_file, &class_path)?;
    nf_project::create_dispatcher(&class_path, &name).map_err(|error| titled(error, "The event dispatcher could not be created"))?;
    reopen(&project_file)
}

#[tauri::command]
pub async fn create_method(project_file: PathBuf, class_path: PathBuf, name: String) -> CommandResult<ProjectView> {
    ensure_inside_project(&project_file, &class_path)?;
    nf_project::create_method(&class_path, &name).map_err(|error| titled(error, "The method could not be created"))?;
    reopen(&project_file)
}

#[tauri::command]
pub async fn create_folder(project_file: PathBuf, location: EntryLocation, name: String) -> CommandResult<ProjectView> {
    ensure_inside_project(&project_file, &location.package_path)?;
    nf_project::create_folder(&location.directory(), &name)
        .map_err(|error| titled(error, "The folder could not be created"))?;
    reopen(&project_file)
}

#[tauri::command]
pub async fn rename_entry(project_file: PathBuf, path: PathBuf, name: String) -> CommandResult<ProjectView> {
    ensure_inside_project(&project_file, &path)?;
    nf_project::rename_entry(&path, &name).map_err(|error| titled(error, "Renaming failed"))?;
    reopen(&project_file)
}

#[tauri::command]
pub async fn move_entry(project_file: PathBuf, path: PathBuf, target: EntryLocation) -> CommandResult<ProjectView> {
    ensure_inside_project(&project_file, &path)?;
    ensure_inside_project(&project_file, &target.package_path)?;
    let source_package = path
        .ancestors()
        .find(|ancestor| ancestor.parent().and_then(Path::file_name).is_some_and(|name| name == Project::PACKAGES_DIRECTORY));
    if source_package != Some(target.package_path.as_path()) {
        return Err(CommandError::new(
            "Cannot move between packages",
            "Every package runs in its own Lua environment, so graphs can only be moved within their package.",
        ));
    }
    let is_method = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == nf_project::METHODS_DIRECTORY)
        && path.parent().and_then(Path::parent).is_some_and(nf_project::is_graph_directory);
    if is_method {
        return Err(CommandError::new(
            "Methods stay with their class",
            "A method belongs to its class and moves together with it. To use the logic elsewhere, create a function instead.",
        ));
    }
    nf_project::move_entry(&path, &target.directory()).map_err(|error| titled(error, "Moving failed"))?;
    reopen(&project_file)
}

#[tauri::command]
pub async fn delete_entry(project_file: PathBuf, path: PathBuf) -> CommandResult<ProjectView> {
    ensure_inside_project(&project_file, &path)?;
    nf_project::delete_entry(&path).map_err(|error| titled(error, "Deleting failed"))?;
    reopen(&project_file)
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LuaPreview {
    pub file: Option<String>,
    pub code: String,
    pub problems: Vec<nf_compiler::BuildProblem>,
}

#[tauri::command]
pub async fn build_project(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    project_file: PathBuf,
) -> CommandResult<nf_compiler::BuildReport> {
    let cached = cache.get(channel)?;
    let project = Project::open(&project_file)?;
    nf_compiler::build_project(&project, &cached.catalog).map_err(|error| titled(error, "The build failed"))
}

#[tauri::command]
pub async fn preview_lua(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_id: Id,
) -> CommandResult<LuaPreview> {
    let cached = cache.get(channel)?;
    let compiled = nf_compiler::compile_package(&cached.catalog, &package_path)
        .map_err(|error| titled(error, "The Lua preview failed"))?;
    let file = compiled.graph_files.get(&graph_id).cloned();
    let code = file
        .as_ref()
        .and_then(|file| compiled.files.get(file))
        .cloned()
        .unwrap_or_default();
    let problems = compiled
        .problems
        .into_iter()
        .filter(|problem| problem.graph.as_ref().is_some_and(|graph| graph.id == graph_id))
        .collect();
    Ok(LuaPreview { file, code, problems })
}

#[tauri::command]
pub async fn check_collapse(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    graph_path: PathBuf,
    selection: Vec<Id>,
) -> CommandResult<Option<nf_graph::CollapseError>> {
    let cached = cache.get(channel)?;
    nf_graph::check_collapse(&cached.catalog, &package_path, &graph_path, &selection)
        .map_err(|error| titled(error, "The graph could not be read"))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollapseResult {
    project: ProjectView,
    function: nf_graph::CollapsedFunction,
}

#[tauri::command]
pub async fn collapse_to_function(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    project_file: PathBuf,
    package_path: PathBuf,
    graph_path: PathBuf,
    selection: Vec<Id>,
    name: String,
) -> CommandResult<CollapseResult> {
    ensure_inside_project(&project_file, &graph_path)?;
    let cached = cache.get(channel)?;
    let function = nf_graph::collapse_into_function(&cached.catalog, &package_path, &graph_path, &selection, &name).map_err(|failure| match failure {
        nf_graph::CollapseFailure::Selection(error) => {
            CommandError::new("The selection cannot be collapsed", format!("{} {}", error.message, error.hint))
        }
        nf_graph::CollapseFailure::Project(error) => titled(error, "The function could not be created"),
    })?;
    Ok(CollapseResult {
        project: reopen(&project_file)?,
        function,
    })
}

#[tauri::command]
pub async fn search_project(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    query: String,
) -> CommandResult<nf_graph::SearchResult> {
    let cached = cache.get(channel)?;
    nf_graph::search_package(&cached.catalog, &package_path, &query).map_err(|error| titled(error, "The search failed"))
}

#[tauri::command]
pub async fn find_usages(
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    package_path: PathBuf,
    target: Id,
) -> CommandResult<nf_graph::Usages> {
    let cached = cache.get(channel)?;
    nf_graph::find_usages(&cached.catalog, &package_path, target).map_err(|error| titled(error, "The usages could not be found"))
}

#[tauri::command]
pub async fn suggest_log_files(project_file: PathBuf) -> CommandResult<Vec<PathBuf>> {
    let project = Project::open(&project_file)?;
    Ok(nf_compiler::suggest_log_files(&project))
}

#[tauri::command]
pub async fn read_runtime_log(project_file: PathBuf, path: PathBuf, offset: Option<u64>) -> CommandResult<nf_compiler::LogChunk> {
    let project = Project::open(&project_file)?;
    let maps = nf_compiler::load_source_maps(&project);
    nf_compiler::read_log(&path, offset, &maps).map_err(|error| titled(error, "The log file could not be read"))
}

#[tauri::command]
pub async fn parse_runtime_log(project_file: PathBuf, text: String) -> CommandResult<Vec<nf_compiler::LogEntry>> {
    let project = Project::open(&project_file)?;
    let maps = nf_compiler::load_source_maps(&project);
    Ok(nf_compiler::parse_log(&text, &maps))
}
