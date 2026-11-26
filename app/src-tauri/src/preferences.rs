use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::debugger::DebuggerState;
use crate::error::{CommandError, CommandResult};

const PREFERENCES_FILE: &str = "preferences.toml";
pub const PREFERENCES_EVENT: &str = "preferences";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum NodeListLayout {
    #[default]
    Flat,
    Categorized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub interface_scale: u16,
    pub button_labels: bool,
    pub node_list: NodeListLayout,
    pub minimap: bool,
    pub debugger_port: u16,
    pub api_auto_update: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            interface_scale: 100,
            button_labels: true,
            node_list: NodeListLayout::Flat,
            minimap: true,
            debugger_port: nf_compiler::DEBUGGER_PORT,
            api_auto_update: true,
        }
    }
}

#[derive(Default)]
pub struct PreferencesState {
    current: Mutex<Preferences>,
}

fn path(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join(PREFERENCES_FILE))
        .map_err(|error| CommandError::new("Settings folder unavailable", error.to_string()))
}

pub fn load(app: &AppHandle) -> Preferences {
    path(app)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

impl PreferencesState {
    pub fn new(preferences: Preferences) -> Self {
        Self {
            current: Mutex::new(preferences),
        }
    }

    pub fn snapshot(&self) -> Preferences {
        self.lock().clone()
    }

    fn lock(&self) -> MutexGuard<'_, Preferences> {
        self.current.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[tauri::command]
pub fn preferences(state: State<'_, PreferencesState>) -> Preferences {
    state.lock().clone()
}

#[tauri::command]
pub fn update_preferences(
    app: AppHandle,
    state: State<'_, PreferencesState>,
    debugger: State<'_, DebuggerState>,
    preferences: Preferences,
) -> CommandResult<Preferences> {
    if preferences.debugger_port < 1024 {
        return Err(CommandError::new("Invalid port", "Pick a debugger port between 1024 and 65535."));
    }
    let mut cleaned = preferences;
    cleaned.interface_scale = cleaned.interface_scale.clamp(80, 150);
    let target = path(&app)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    }
    let text = toml::to_string(&cleaned).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    std::fs::write(&target, text).map_err(|error| CommandError::new("Settings not saved", error.to_string()))?;
    debugger.restart(&app, cleaned.debugger_port);
    let enabled_updates = cleaned.api_auto_update && !state.lock().api_auto_update;
    *state.lock() = cleaned.clone();
    if enabled_updates {
        crate::api_updates::check_in_background(&app);
    }
    let _ = app.emit(PREFERENCES_EVENT, &cleaned);
    Ok(cleaned)
}
