use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fs_util::write_toml;
use crate::merge::has_conflict_markers;
use serde::de::DeserializeOwned;

use crate::{Id, NodeDocument, ProjectError, VariableDocument};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScriptContext {
    Server,
    Client,
    Shared,
}

impl ScriptContext {
    pub const ALL: [ScriptContext; 3] = [ScriptContext::Server, ScriptContext::Client, ScriptContext::Shared];

    pub fn lua_directory(self) -> &'static str {
        match self {
            ScriptContext::Server => "Server",
            ScriptContext::Client => "Client",
            ScriptContext::Shared => "Shared",
        }
    }

    pub fn display_name(self) -> &'static str {
        self.lua_directory()
    }

    pub fn directory_name(self) -> &'static str {
        match self {
            ScriptContext::Server => "server",
            ScriptContext::Client => "client",
            ScriptContext::Shared => "shared",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphKind {
    EventGraph,
    Function,
    Class,
    Event,
    Struct,
    Macro,
    Enum,
    Dispatcher,
    Interface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EventDirection {
    #[default]
    ToServer,
    ToClients,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionPin {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionSignature {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pure: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exported: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<FunctionPin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub returns: Vec<FunctionPin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceFunction {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<FunctionPin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub returns: Vec<FunctionPin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphManifest {
    pub id: Id,
    pub kind: GraphKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<EventDirection>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functions: Vec<InterfaceFunction>,
    #[serde(flatten)]
    pub signature: FunctionSignature,
}

impl GraphManifest {
    pub const FILE_NAME: &'static str = "graph.toml";
    pub const NODES_DIRECTORY: &'static str = "nodes";

    pub fn new(kind: GraphKind) -> GraphManifest {
        GraphManifest {
            id: Id::generate(),
            kind,
            parent: None,
            direction: (kind == GraphKind::Event).then_some(EventDirection::default()),
            interfaces: Vec::new(),
            functions: Vec::new(),
            signature: FunctionSignature::default(),
        }
    }

    pub fn read(directory: &Path) -> Result<GraphManifest, ProjectError> {
        let path = directory.join(Self::FILE_NAME);
        let content = std::fs::read_to_string(&path).map_err(ProjectError::io(&path))?;
        let readable = match crate::merge::split_conflict(&content) {
            Some(sides) => sides.ours,
            None => content,
        };
        toml::from_str(&readable).map_err(|source| ProjectError::Deserialize { path, source })
    }

    pub fn write(&self, directory: &Path) -> Result<(), ProjectError> {
        write_toml(&directory.join(Self::FILE_NAME), self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeLoadError {
    pub file: String,
    pub message: String,
    pub has_conflict_markers: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDocument {
    pub manifest: GraphManifest,
    pub nodes: Vec<NodeDocument>,
    pub variables: Vec<VariableDocument>,
    pub load_errors: Vec<NodeLoadError>,
}


trait Identified {
    fn identifier(&self) -> Id;
}

impl Identified for NodeDocument {
    fn identifier(&self) -> Id {
        self.id
    }
}

impl Identified for VariableDocument {
    fn identifier(&self) -> Id {
        self.id
    }
}

fn load_documents<T: DeserializeOwned + Identified>(
    graph_directory: &Path,
    subdirectory: &str,
    noun: &str,
    errors: &mut Vec<NodeLoadError>,
) -> Result<Vec<T>, ProjectError> {
    let directory = graph_directory.join(subdirectory);
    let mut files = match std::fs::read_dir(&directory) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(ProjectError::io(&directory)(error)),
    };
    files.sort();

    let mut documents: Vec<T> = Vec::with_capacity(files.len());
    for path in files {
        let file = format!(
            "{subdirectory}/{}",
            path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
        );
        let content = std::fs::read_to_string(&path).map_err(ProjectError::io(&path))?;
        if has_conflict_markers(&content) {
            errors.push(NodeLoadError {
                file,
                message: format!("This {noun} file contains unresolved merge conflict markers. Resolve the conflict in your version control tool and reload the graph."),
                has_conflict_markers: true,
            });
            continue;
        }
        match toml::from_str::<T>(&content) {
            Ok(document) if documents.iter().any(|existing| existing.identifier() == document.identifier()) => {
                errors.push(NodeLoadError {
                    file,
                    message: format!(
                        "Another {noun} file already uses the id {}. This file was skipped; delete it or give it a new id.",
                        document.identifier()
                    ),
                    has_conflict_markers: false,
                });
            }
            Ok(document) => documents.push(document),
            Err(error) => errors.push(NodeLoadError {
                file,
                message: format!("The {noun} file could not be read: {}", error.message()),
                has_conflict_markers: false,
            }),
        }
    }
    Ok(documents)
}

impl GraphDocument {
    pub fn load(directory: &Path) -> Result<GraphDocument, ProjectError> {
        let manifest = GraphManifest::read(directory)?;
        let mut load_errors = Vec::new();
        let manifest_text = std::fs::read_to_string(directory.join(GraphManifest::FILE_NAME)).unwrap_or_default();
        if has_conflict_markers(&manifest_text) {
            load_errors.push(NodeLoadError {
                file: GraphManifest::FILE_NAME.to_owned(),
                message: "The graph settings (name of pins, signature, parent class) contain a merge conflict. Until it is resolved, your version is used.".to_owned(),
                has_conflict_markers: true,
            });
        }
        let nodes = load_documents(directory, GraphManifest::NODES_DIRECTORY, "node", &mut load_errors)?;
        let variables = load_documents(directory, VariableDocument::DIRECTORY, "variable", &mut load_errors)?;
        Ok(GraphDocument {
            manifest,
            nodes,
            variables,
            load_errors,
        })
    }
}

pub fn load_variables(directory: &Path) -> Result<Vec<VariableDocument>, ProjectError> {
    load_documents(directory, VariableDocument::DIRECTORY, "variable", &mut Vec::new())
}

fn ensure_graph_exists(directory: &Path) -> Result<(), ProjectError> {
    if directory.join(GraphManifest::FILE_NAME).is_file() {
        return Ok(());
    }
    Err(ProjectError::io(directory)(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "the graph folder no longer exists",
    )))
}

fn remove_if_present(path: &Path) -> Result<(), ProjectError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(ProjectError::io(path)(error)),
    }
}

pub fn apply_graph_changes(
    directory: &Path,
    upserts: &[NodeDocument],
    deletions: &[Id],
) -> Result<(), ProjectError> {
    ensure_graph_exists(directory)?;
    for node in upserts {
        node.write(directory)?;
    }
    for id in deletions {
        remove_if_present(&NodeDocument::path_for(directory, *id))?;
    }
    Ok(())
}

pub fn apply_variable_changes(
    directory: &Path,
    upserts: &[VariableDocument],
    deletions: &[Id],
) -> Result<(), ProjectError> {
    ensure_graph_exists(directory)?;
    for variable in upserts {
        variable.write(directory)?;
    }
    for id in deletions {
        remove_if_present(&VariableDocument::path_for(directory, *id))?;
    }
    Ok(())
}

pub fn update_interface(directory: &Path, description: &str, functions: Vec<InterfaceFunction>) -> Result<GraphManifest, ProjectError> {
    let mut manifest = GraphManifest::read(directory)?;
    manifest.signature.description = description.trim().to_owned();
    manifest.functions = functions;
    manifest.write(directory)?;
    Ok(manifest)
}

pub struct ClassSettings<'a> {
    pub parent: &'a str,
    pub description: &'a str,
    pub interfaces: &'a [String],
    pub exported: bool,
}

pub fn update_class(directory: &Path, settings: ClassSettings<'_>) -> Result<GraphManifest, ProjectError> {
    let ClassSettings { parent, description, interfaces, exported } = settings;
    let mut manifest = GraphManifest::read(directory)?;
    manifest.signature.exported = exported;
    manifest.parent = Some(parent.to_owned());
    manifest.interfaces = interfaces.to_vec();
    manifest.signature.description = description.trim().to_owned();
    manifest.write(directory)?;
    Ok(manifest)
}

pub fn update_event(directory: &Path, direction: EventDirection, signature: FunctionSignature) -> Result<GraphManifest, ProjectError> {
    let mut manifest = GraphManifest::read(directory)?;
    manifest.direction = Some(direction);
    manifest.signature = FunctionSignature {
        pure: false,
        returns: Vec::new(),
        ..signature
    };
    manifest.write(directory)?;
    Ok(manifest)
}

pub fn update_function_signature(directory: &Path, signature: FunctionSignature) -> Result<GraphManifest, ProjectError> {
    let mut manifest = GraphManifest::read(directory)?;
    manifest.signature = signature;
    manifest.write(directory)?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PinLink, PinValue, create_graph};

    #[test]
    fn saves_and_loads_node_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = create_graph(&temporary.path().join("server"), "Main", GraphKind::EventGraph).unwrap();

        let event = NodeDocument::new("event/Package/Load", [0.0, 0.0]);
        let mut call = NodeDocument::new("static/Console/Log", [300.0, 0.0]);
        call.inputs.insert(
            "message".to_owned(),
            PinValue::Literal {
                literal: toml::Value::String("Hello".to_owned()),
            },
        );
        let mut event_with_link = event.clone();
        event_with_link.exec.insert(
            "then".to_owned(),
            PinLink {
                node: call.id,
                pin: "exec".to_owned(),
            },
        );
        apply_graph_changes(&directory, &[event_with_link.clone(), call.clone()], &[]).unwrap();

        let loaded = GraphDocument::load(&directory).unwrap();
        assert!(loaded.load_errors.is_empty());
        assert_eq!(loaded.nodes.len(), 2);
        assert!(loaded.nodes.contains(&event_with_link));
        assert!(loaded.nodes.contains(&call));

        apply_graph_changes(&directory, &[], &[call.id]).unwrap();
        assert_eq!(GraphDocument::load(&directory).unwrap().nodes.len(), 1);
    }

    #[test]
    fn reports_conflicted_node_files_without_failing() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = create_graph(&temporary.path().join("client"), "Main", GraphKind::EventGraph).unwrap();
        let node = NodeDocument::new("event/Package/Load", [0.0, 0.0]);
        node.write(&directory).unwrap();
        std::fs::write(
            directory.join("nodes").join("broken.toml"),
            "<<<<<<< HEAD\nposition = [0.0, 0.0]\n=======\nposition = [5.0, 0.0]\n>>>>>>> other\n",
        )
        .unwrap();

        let loaded = GraphDocument::load(&directory).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.load_errors.len(), 1);
        assert!(loaded.load_errors[0].has_conflict_markers);
    }
}
