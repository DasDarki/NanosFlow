use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nf_api::{ApiChannel, ApiDatabase};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::graph_commands::CatalogCache;
use crate::preferences::PreferencesState;

const DIRECTORY: &str = "api";
const CURRENT_FILE: &str = "current";
const CHECKED_FILE: &str = "checked";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
pub const API_UPDATED_EVENT: &str = "api-updated";

#[derive(Default)]
pub struct ApiUpdateState {
    running: Mutex<bool>,
    error: Mutex<Option<String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiStatus {
    revision: String,
    bundled_revision: String,
    downloaded: bool,
    last_check: Option<u64>,
    checking: bool,
    error: Option<String>,
    repository: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiUpdated {
    revision: String,
    previous: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs()).unwrap_or_default()
}

fn root(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|directory| directory.join(DIRECTORY))
}

fn is_valid(snapshot: &Path) -> bool {
    ApiChannel::ALL.iter().all(|channel| ApiDatabase::from_snapshot(snapshot, *channel).is_ok())
}

fn current_snapshot(root: &Path) -> Option<PathBuf> {
    let revision = std::fs::read_to_string(root.join(CURRENT_FILE)).ok()?;
    let snapshot = root.join(revision.trim());
    snapshot.is_dir().then_some(snapshot)
}

fn last_check(root: &Path) -> Option<u64> {
    std::fs::read_to_string(root.join(CHECKED_FILE)).ok()?.trim().parse().ok()
}

pub fn restore(app: &AppHandle) {
    let Some(root) = root(app) else {
        return;
    };
    if let Some(snapshot) = current_snapshot(&root).filter(|snapshot| is_valid(snapshot)) {
        app.state::<CatalogCache>().use_snapshot(Some(snapshot));
    }
}

fn status(app: &AppHandle) -> ApiStatus {
    let state = app.state::<ApiUpdateState>();
    let cache = app.state::<CatalogCache>();
    ApiStatus {
        revision: cache.revision(),
        bundled_revision: ApiDatabase::bundled_revision(),
        downloaded: cache.snapshot().is_some(),
        last_check: root(app).as_deref().and_then(last_check),
        checking: state.running.lock().map(|running| *running).unwrap_or(false),
        error: state.error.lock().ok().and_then(|error| error.clone()),
        repository: nf_api::snapshot::REPOSITORY,
    }
}

fn update(app: &AppHandle) -> Result<(), String> {
    let root = root(app).ok_or_else(|| "The app data folder is not available.".to_owned())?;
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let latest = nf_api::snapshot::latest_revision().map_err(|error| format!("GitHub could not be reached: {error}"))?;
    let _ = std::fs::write(root.join(CHECKED_FILE), now().to_string());
    let cache = app.state::<CatalogCache>();
    let previous = cache.revision();
    if previous == latest {
        return Ok(());
    }

    let snapshot = root.join(&latest);
    if !is_valid(&snapshot) {
        let staging = root.join(format!("{latest}.partial"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
        let downloaded = nf_api::snapshot::download(&latest, &staging).map_err(|error| format!("The API could not be downloaded: {error}"));
        if let Err(error) = downloaded.and_then(|()| {
            if is_valid(&staging) {
                Ok(())
            } else {
                Err("The downloaded API could not be read by this version of NanosFlow. Update NanosFlow to use it.".to_owned())
            }
        }) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(error);
        }
        let _ = std::fs::remove_dir_all(&snapshot);
        std::fs::rename(&staging, &snapshot).map_err(|error| error.to_string())?;
    }
    std::fs::write(root.join(CURRENT_FILE), &latest).map_err(|error| error.to_string())?;
    cache.use_snapshot(Some(snapshot));
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && name != latest {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let _ = app.emit(API_UPDATED_EVENT, ApiUpdated { revision: latest, previous });
    Ok(())
}

fn run_check(app: &AppHandle) {
    let state = app.state::<ApiUpdateState>();
    {
        let Ok(mut running) = state.running.lock() else {
            return;
        };
        if *running {
            return;
        }
        *running = true;
    }
    let result = update(app);
    if let Ok(mut error) = state.error.lock() {
        *error = result.err();
    }
    if let Ok(mut running) = state.running.lock() {
        *running = false;
    }
}

pub fn check_in_background(app: &AppHandle) {
    let enabled = app.state::<PreferencesState>().snapshot().api_auto_update;
    let due = root(app)
        .as_deref()
        .and_then(last_check)
        .is_none_or(|checked| now().saturating_sub(checked) >= CHECK_INTERVAL.as_secs());
    if !enabled || !due {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || run_check(&app));
}

#[tauri::command]
pub fn api_status(app: AppHandle) -> ApiStatus {
    status(&app)
}

#[tauri::command]
pub async fn check_api_updates(app: AppHandle, _state: State<'_, ApiUpdateState>) -> CommandResult<ApiStatus> {
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_check(&worker))
        .await
        .map_err(|error| CommandError::new("Update check failed", error.to_string()))?;
    Ok(status(&app))
}

#[tauri::command]
pub fn use_bundled_api(app: AppHandle) -> CommandResult<ApiStatus> {
    if let Some(root) = root(&app) {
        let _ = std::fs::remove_file(root.join(CURRENT_FILE));
    }
    let cache = app.state::<CatalogCache>();
    let previous = cache.revision();
    cache.use_snapshot(None);
    let _ = app.emit(API_UPDATED_EVENT, ApiUpdated { revision: cache.revision(), previous });
    Ok(status(&app))
}
