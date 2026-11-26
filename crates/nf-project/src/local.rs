use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_util::{read_toml, write_toml};
use crate::{Project, ProjectError};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SteamApp {
    #[default]
    Playtest,
    Game,
}

impl SteamApp {
    pub fn argument(self) -> &'static str {
        match self {
            SteamApp::Playtest => "playtest",
            SteamApp::Game => "game",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServerBranch {
    #[default]
    Public,
    BleedingEdge,
}

impl ServerBranch {
    pub fn steam_branch(self) -> &'static str {
        match self {
            ServerBranch::Public => "public",
            ServerBranch::BleedingEdge => "bleeding-edge",
        }
    }
}

pub const DEFAULT_SERVER_PORT: u16 = 7777;

fn default_port() -> u16 {
    DEFAULT_SERVER_PORT
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_default_port(port: &u16) -> bool {
    *port == DEFAULT_SERVER_PORT
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TestServerSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<PathBuf>,
    #[serde(default)]
    pub steam_app: SteamApp,
    #[serde(default)]
    pub branch: ServerBranch,
    #[serde(default = "default_port", skip_serializing_if = "is_default_port")]
    pub port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_mode: Option<String>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub download_missing: bool,
}

impl Default for TestServerSettings {
    fn default() -> TestServerSettings {
        TestServerSettings {
            directory: None,
            steam_app: SteamApp::default(),
            branch: ServerBranch::default(),
            port: DEFAULT_SERVER_PORT,
            map: None,
            game_mode: None,
            download_missing: true,
        }
    }
}

impl TestServerSettings {
    fn is_default(&self) -> bool {
        *self == TestServerSettings::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_directory: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "TestServerSettings::is_default")]
    pub test_server: TestServerSettings,
}

impl LocalSettings {
    pub const FILE_NAME: &'static str = "settings.toml";

    pub fn path(project_root: &Path) -> PathBuf {
        project_root.join(Project::LOCAL_DIRECTORY).join(Self::FILE_NAME)
    }

    pub fn read(project_root: &Path) -> Result<LocalSettings, ProjectError> {
        let path = Self::path(project_root);
        if !path.is_file() {
            return Ok(LocalSettings::default());
        }
        read_toml(&path)
    }

    pub fn write(&self, project_root: &Path) -> Result<(), ProjectError> {
        write_toml(&Self::path(project_root), self)
    }
}
