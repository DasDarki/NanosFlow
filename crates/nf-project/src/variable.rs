use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_util::write_toml;
use crate::{Id, ProjectError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableDocument {
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<toml::Value>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sync: bool,
}

impl VariableDocument {
    pub const DIRECTORY: &'static str = "variables";

    pub fn path_for(graph_directory: &Path, id: Id) -> PathBuf {
        graph_directory.join(Self::DIRECTORY).join(format!("{id}.toml"))
    }

    pub fn write(&self, graph_directory: &Path) -> Result<(), ProjectError> {
        write_toml(&Self::path_for(graph_directory, self.id), self)
    }
}
