use std::path::Path;

use nf_api::ApiChannel;
use serde::{Deserialize, Serialize};

use crate::fs_util::{read_toml, write_toml};
use crate::{Id, ProjectError};

pub const PROJECT_EXTENSION: &str = "nfproj";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub format: u32,
    pub id: Id,
    pub name: String,
    #[serde(default)]
    pub api_channel: ApiChannel,
    #[serde(default)]
    pub build: BuildSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildSettings {
    pub output_directory: String,
}

impl Default for BuildSettings {
    fn default() -> BuildSettings {
        BuildSettings {
            output_directory: "build".to_owned(),
        }
    }
}

impl ProjectManifest {
    pub fn new(name: impl Into<String>, api_channel: ApiChannel) -> ProjectManifest {
        ProjectManifest {
            format: FORMAT_VERSION,
            id: Id::generate(),
            name: name.into(),
            api_channel,
            build: BuildSettings::default(),
        }
    }

    pub fn read(path: &Path) -> Result<ProjectManifest, ProjectError> {
        let has_extension = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_EXTENSION));
        if !has_extension {
            return Err(ProjectError::NotAProjectFile {
                path: path.to_path_buf(),
                expected: PROJECT_EXTENSION,
            });
        }

        let manifest: ProjectManifest = read_toml(path)?;
        if manifest.format > FORMAT_VERSION {
            return Err(ProjectError::UnsupportedFormat {
                path: path.to_path_buf(),
                found: manifest.format,
                supported: FORMAT_VERSION,
            });
        }
        Ok(manifest)
    }

    pub fn write(&self, path: &Path) -> Result<(), ProjectError> {
        write_toml(path, self)
    }
}
