use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_util::{read_toml, write_toml};
use crate::{GraphManifest, Id, ProjectError};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PinLink {
    pub node: Id,
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PinValue {
    Link(PinLink),
    Literal { literal: toml::Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDocument {
    pub id: Id,
    #[serde(rename = "type", alias = "node_type")]
    pub node_type: String,
    pub position: [f64; 2],
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, PinValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exec: BTreeMap<String, PinLink>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, toml::Value>,
}

impl NodeDocument {
    pub const EXTENSION: &'static str = "toml";

    pub fn new(node_type: impl Into<String>, position: [f64; 2]) -> NodeDocument {
        NodeDocument {
            id: Id::generate(),
            node_type: node_type.into(),
            position,
            inputs: BTreeMap::new(),
            exec: BTreeMap::new(),
            properties: BTreeMap::new(),
        }
    }

    pub fn path_for(graph_directory: &Path, id: Id) -> PathBuf {
        graph_directory
            .join(GraphManifest::NODES_DIRECTORY)
            .join(format!("{id}.{}", Self::EXTENSION))
    }

    pub fn path_in(&self, graph_directory: &Path) -> PathBuf {
        Self::path_for(graph_directory, self.id)
    }

    pub fn read(path: &Path) -> Result<NodeDocument, ProjectError> {
        read_toml(path)
    }

    pub fn write(&self, graph_directory: &Path) -> Result<(), ProjectError> {
        write_toml(&self.path_in(graph_directory), self)
    }
}
