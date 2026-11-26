use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MAXIMUM_ENTRIES: usize = 20;
const FILE_NAME: &str = "recent-projects.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub path: PathBuf,
    pub name: String,
    pub last_opened: u64,
    #[serde(skip_deserializing, default)]
    pub exists: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RecentProjectsFile {
    #[serde(default)]
    projects: Vec<RecentProject>,
}

pub struct RecentProjects {
    file: PathBuf,
}

impl RecentProjects {
    pub fn new(config_directory: &Path) -> RecentProjects {
        RecentProjects {
            file: config_directory.join(FILE_NAME),
        }
    }

    fn load(&self) -> RecentProjectsFile {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save(&self, recent: &RecentProjectsFile) -> std::io::Result<()> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(recent).map_err(std::io::Error::other)?;
        std::fs::write(&self.file, content)
    }

    pub fn list(&self) -> Vec<RecentProject> {
        let mut projects = self.load().projects;
        for project in &mut projects {
            project.exists = project.path.is_file();
        }
        projects
    }

    pub fn touch(&self, path: &Path, name: &str) -> std::io::Result<()> {
        let mut recent = self.load();
        recent.projects.retain(|project| project.path != path);
        let last_opened = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        recent.projects.insert(
            0,
            RecentProject {
                path: path.to_path_buf(),
                name: name.to_owned(),
                last_opened,
                exists: true,
            },
        );
        recent.projects.truncate(MAXIMUM_ENTRIES);
        self.save(&recent)
    }

    pub fn remove(&self, path: &Path) -> std::io::Result<()> {
        let mut recent = self.load();
        recent.projects.retain(|project| project.path != path);
        self.save(&recent)
    }
}
