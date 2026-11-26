use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use nf_api::ApiChannel;
use nf_project::{
    DEFAULT_COMPATIBILITY_VERSION, FieldIssue, NewProjectOptions, PackageSummary, Project,
    ProjectTemplate, ScriptContext,
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::recent::{RecentProject, RecentProjects};

pub struct LaunchState {
    pub pending_project: Mutex<Option<PathBuf>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub project_file: PathBuf,
    pub root: PathBuf,
    pub api_channel: ApiChannel,
    pub packages: Vec<PackageSummary>,
    pub output_directory: PathBuf,
    pub custom_output_directory: Option<PathBuf>,
}

impl From<&Project> for ProjectView {
    fn from(project: &Project) -> ProjectView {
        ProjectView {
            id: project.manifest().id.to_string(),
            name: project.manifest().name.clone(),
            project_file: project.project_file().to_path_buf(),
            root: project.root().to_path_buf(),
            api_channel: project.manifest().api_channel,
            packages: project.packages().to_vec(),
            output_directory: project.output_directory(),
            custom_output_directory: project.local_settings().output_directory.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedProject {
    pub project: ProjectView,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub api_revision: String,
    pub platform: String,
}

fn recent_projects(app: &AppHandle) -> CommandResult<RecentProjects> {
    let directory = app.path().app_config_dir().map_err(|error| {
        CommandError::new("Settings folder unavailable", error.to_string())
    })?;
    Ok(RecentProjects::new(&directory))
}

fn remember(app: &AppHandle, project: &Project) {
    if crate::session::is_session_copy(app, project.root()) {
        return;
    }
    if let Ok(recent) = recent_projects(app) {
        let _ = recent.touch(project.project_file(), &project.manifest().name);
    }
}

fn default_author() -> String {
    let from_git = Command::new("git")
        .args(["config", "--global", "user.name"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|name| !name.is_empty());
    from_git
        .or_else(|| std::env::var("USERNAME").ok())
        .or_else(|| std::env::var("USER").ok())
        .unwrap_or_default()
}

fn default_location(app: &AppHandle) -> PathBuf {
    let base = app
        .path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_default();
    base.join("NanosFlow Projects")
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    let api_revision = nf_api::ApiDatabase::bundled(ApiChannel::Stable)
        .map(|database| database.revision().to_owned())
        .unwrap_or_else(|_| "unavailable".to_owned());
    AppInfo {
        version: app.package_info().version.to_string(),
        api_revision,
        platform: std::env::consts::OS.to_owned(),
    }
}

#[tauri::command]
pub fn default_project_options(app: AppHandle) -> NewProjectOptions {
    let location = default_location(&app);
    let _ = std::fs::create_dir_all(&location);
    NewProjectOptions {
        name: String::new(),
        location,
        folder_name: String::new(),
        template: ProjectTemplate::GameMode,
        package_id: String::new(),
        package_type: ProjectTemplate::GameMode.package_type(),
        author: default_author(),
        version: "0.1.0".to_owned(),
        compatibility_version: DEFAULT_COMPATIBILITY_VERSION.to_owned(),
        contexts: vec![ScriptContext::Server, ScriptContext::Client],
        api_channel: ApiChannel::Stable,
        map_asset: None,
        initialize_git: true,
    }
}

#[tauri::command]
pub fn slugify(value: String) -> String {
    nf_project::slugify(&value)
}

#[tauri::command]
pub fn validate_new_project(options: NewProjectOptions) -> Vec<FieldIssue> {
    nf_project::validate_new_project(&options)
}

#[tauri::command]
pub async fn create_project(app: AppHandle, options: NewProjectOptions) -> CommandResult<CreatedProject> {
    let (project, warnings) = nf_project::create_project(&options)?;
    remember(&app, &project);
    Ok(CreatedProject {
        project: ProjectView::from(&project),
        warnings,
    })
}

#[tauri::command]
pub async fn open_project(app: AppHandle, path: PathBuf) -> CommandResult<ProjectView> {
    if !path.is_file() {
        return Err(CommandError::new(
            "Project not found",
            format!("'{}' does not exist anymore. It may have been moved or deleted.", path.display()),
        ));
    }
    let project = Project::open(&path)?;
    remember(&app, &project);
    Ok(ProjectView::from(&project))
}

#[tauri::command]
pub fn list_recent_projects(app: AppHandle) -> CommandResult<Vec<RecentProject>> {
    Ok(recent_projects(&app)?.list())
}

#[tauri::command]
pub fn remove_recent_project(app: AppHandle, path: PathBuf) -> CommandResult<()> {
    recent_projects(&app)?
        .remove(&path)
        .map_err(|error| CommandError::new("Could not update recent projects", error.to_string()))
}

#[tauri::command]
pub fn take_launch_project(state: State<'_, LaunchState>) -> Option<PathBuf> {
    state
        .pending_project
        .lock()
        .ok()
        .and_then(|mut pending| pending.take())
}

#[tauri::command]
pub fn path_exists(path: PathBuf) -> bool {
    Path::new(&path).exists()
}

fn resolve_output_directory(project: &Project, chosen: PathBuf) -> CommandResult<PathBuf> {
    if !chosen.is_absolute() || !chosen.is_dir() {
        return Err(CommandError::new(
            "Folder not found",
            format!("'{}' is not an existing folder.", chosen.display()),
        ));
    }
    let server_packages = chosen.join("Packages");
    let resolved = if chosen.join("Config.toml").is_file() && server_packages.is_dir() {
        server_packages
    } else {
        chosen
    };
    let sources = project.root().join(Project::PACKAGES_DIRECTORY);
    let canonical = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
    let canonical_sources = sources.canonicalize().unwrap_or(sources);
    if canonical.starts_with(&canonical_sources) {
        return Err(CommandError::new(
            "Folder not allowed",
            "The build output cannot go into the project's own packages folder, because it would mix generated Lua with your graphs.",
        ));
    }
    Ok(resolved)
}

#[tauri::command]
pub async fn update_output_directory(project_file: PathBuf, directory: Option<PathBuf>) -> CommandResult<ProjectView> {
    let project = Project::open(&project_file)?;
    let mut settings = project.local_settings().clone();
    settings.output_directory = match directory {
        Some(chosen) => Some(resolve_output_directory(&project, chosen)?),
        None => None,
    };
    settings.write(project.root())?;
    let project = Project::open(&project_file)?;
    Ok(ProjectView::from(&project))
}
