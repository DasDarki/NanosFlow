use std::path::{Component, Path};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::fs_util::sorted_files;
use crate::{GraphManifest, NodeDocument, ProjectError, VariableDocument};

const OURS_MARKER: &str = "<<<<<<<";
const BASE_MARKER: &str = "|||||||";
const SPLIT_MARKER: &str = "=======";
const THEIRS_MARKER: &str = ">>>>>>>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSides {
    pub ours: String,
    pub theirs: String,
    pub ours_label: String,
    pub theirs_label: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Common,
    Ours,
    Base,
    Theirs,
}

pub fn has_conflict_markers(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.starts_with(&format!("{OURS_MARKER} ")) || line == SPLIT_MARKER || line.starts_with(&format!("{THEIRS_MARKER} ")))
}

pub fn split_conflict(content: &str) -> Option<ConflictSides> {
    let mut sides = ConflictSides {
        ours: String::new(),
        theirs: String::new(),
        ours_label: String::new(),
        theirs_label: String::new(),
    };
    let mut section = Section::Common;
    let mut found = false;
    for line in content.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if let Some(label) = bare.strip_prefix(OURS_MARKER).filter(|_| section == Section::Common) {
            section = Section::Ours;
            found = true;
            if sides.ours_label.is_empty() {
                sides.ours_label = label.trim().to_owned();
            }
            continue;
        }
        if bare.starts_with(BASE_MARKER) && section == Section::Ours {
            section = Section::Base;
            continue;
        }
        if bare == SPLIT_MARKER && matches!(section, Section::Ours | Section::Base) {
            section = Section::Theirs;
            continue;
        }
        if let Some(label) = bare.strip_prefix(THEIRS_MARKER).filter(|_| section == Section::Theirs) {
            section = Section::Common;
            if sides.theirs_label.is_empty() {
                sides.theirs_label = label.trim().to_owned();
            }
            continue;
        }
        match section {
            Section::Common => {
                sides.ours.push_str(line);
                sides.theirs.push_str(line);
            }
            Section::Ours => sides.ours.push_str(line),
            Section::Theirs => sides.theirs.push_str(line),
            Section::Base => {}
        }
    }
    (found && section == Section::Common).then_some(sides)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictKind {
    Node,
    Variable,
    Graph,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ConflictSide {
    Node { node: NodeDocument },
    Variable { variable: VariableDocument },
    Graph { manifest: GraphManifest },
    Deleted,
    Unreadable { text: String, error: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileConflict {
    pub file: String,
    pub kind: ConflictKind,
    pub ours_label: String,
    pub theirs_label: String,
    pub ours: ConflictSide,
    pub theirs: ConflictSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConflictChoice {
    Ours,
    Theirs,
}

fn side(kind: ConflictKind, text: &str) -> ConflictSide {
    if text.trim().is_empty() {
        return ConflictSide::Deleted;
    }
    let unreadable = |error: toml::de::Error| ConflictSide::Unreadable {
        text: text.to_owned(),
        error: error.message().to_owned(),
    };
    match kind {
        ConflictKind::Node => toml::from_str(text).map(|node| ConflictSide::Node { node }).unwrap_or_else(unreadable),
        ConflictKind::Variable => toml::from_str(text).map(|variable| ConflictSide::Variable { variable }).unwrap_or_else(unreadable),
        ConflictKind::Graph => toml::from_str(text).map(|manifest| ConflictSide::Graph { manifest }).unwrap_or_else(unreadable),
    }
}

fn conflicted_files(graph_directory: &Path) -> Result<Vec<(String, ConflictKind)>, ProjectError> {
    let mut files = vec![(GraphManifest::FILE_NAME.to_owned(), ConflictKind::Graph)];
    for (directory, kind) in [(GraphManifest::NODES_DIRECTORY, ConflictKind::Node), (VariableDocument::DIRECTORY, ConflictKind::Variable)] {
        for path in sorted_files(&graph_directory.join(directory), "toml")? {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            files.push((format!("{directory}/{name}"), kind));
        }
    }
    Ok(files)
}

pub fn graph_conflicts(graph_directory: &Path) -> Result<Vec<FileConflict>, ProjectError> {
    let mut conflicts = Vec::new();
    for (file, kind) in conflicted_files(graph_directory)? {
        let path = graph_directory.join(&file);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !has_conflict_markers(&content) {
            continue;
        }
        let Some(sides) = split_conflict(&content) else {
            continue;
        };
        conflicts.push(FileConflict {
            file,
            kind,
            ours: side(kind, &sides.ours),
            theirs: side(kind, &sides.theirs),
            ours_label: sides.ours_label,
            theirs_label: sides.theirs_label,
        });
    }
    Ok(conflicts)
}

fn is_inside(file: &str) -> bool {
    let path = Path::new(file);
    path.components().all(|component| matches!(component, Component::Normal(_))) && path.extension().is_some_and(|extension| extension == "toml")
}

pub fn resolve_conflict(graph_directory: &Path, file: &str, choice: ConflictChoice) -> Result<(), ProjectError> {
    let invalid = |message: &str| ProjectError::InvalidOptions {
        issues: vec![crate::FieldIssue::error("file", message)],
    };
    if !is_inside(file) {
        return Err(invalid("This file is not part of the graph."));
    }
    let path = graph_directory.join(file);
    let content = std::fs::read_to_string(&path).map_err(ProjectError::io(&path))?;
    let sides = split_conflict(&content).ok_or_else(|| invalid("This file no longer contains a conflict. Reload the graph."))?;
    let chosen = match choice {
        ConflictChoice::Ours => sides.ours,
        ConflictChoice::Theirs => sides.theirs,
    };
    if chosen.trim().is_empty() {
        if file == GraphManifest::FILE_NAME {
            return Err(invalid("The graph file cannot be deleted. Choose the other version."));
        }
        return std::fs::remove_file(&path).map_err(ProjectError::io(&path));
    }
    std::fs::write(&path, chosen).map_err(ProjectError::io(&path))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommittedGraph {
    pub revision: String,
    pub summary: String,
    pub nodes: Vec<NodeDocument>,
}

fn git(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn committed_graph(graph_directory: &Path) -> Option<CommittedGraph> {
    let revision = git(graph_directory, &["rev-parse", "--short", "HEAD"])?.trim().to_owned();
    let summary = git(graph_directory, &["log", "-1", "--format=%s"]).unwrap_or_default().trim().to_owned();
    let listing = git(graph_directory, &["ls-tree", "--name-only", "HEAD", &format!("{}/", GraphManifest::NODES_DIRECTORY)]).unwrap_or_default();
    let mut nodes = Vec::new();
    for line in listing.lines().filter(|line| line.ends_with(".toml")) {
        let name = Path::new(line).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(content) = git(graph_directory, &["show", &format!("HEAD:./{}/{name}", GraphManifest::NODES_DIRECTORY)]) else {
            continue;
        };
        if let Ok(node) = toml::from_str::<NodeDocument>(&content) {
            nodes.push(node);
        }
    }
    Some(CommittedGraph { revision, summary, nodes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GraphKind, create_graph};

    const CONFLICTED: &str = "id = \"01jabcdefghjkmnpqrstvwxyz0\"\ntype = \"debug/print\"\n<<<<<<< HEAD\nposition = [0.0, 0.0]\n||||||| base\nposition = [5.0, 5.0]\n=======\nposition = [100.0, 40.0]\n>>>>>>> feature/move\n";

    #[test]
    fn splits_both_sides_including_diff3_bases() {
        let sides = split_conflict(CONFLICTED).unwrap();
        assert!(sides.ours.contains("[0.0, 0.0]") && !sides.ours.contains("5.0"));
        assert!(sides.theirs.contains("[100.0, 40.0]") && sides.theirs.starts_with("id = "));
        assert_eq!((sides.ours_label.as_str(), sides.theirs_label.as_str()), ("HEAD", "feature/move"));
        assert_eq!(split_conflict("id = 1\n"), None);
    }

    #[test]
    fn lists_and_resolves_conflicted_files() {
        let temporary = tempfile::tempdir().unwrap();
        let graph = create_graph(temporary.path(), "Main", GraphKind::EventGraph).unwrap();
        let file = "nodes/01jabcdefghjkmnpqrstvwxyz0.toml";
        std::fs::write(graph.join(file), CONFLICTED).unwrap();
        let deleted = "nodes/01jabcdefghjkmnpqrstvwxyz1.toml";
        std::fs::write(graph.join(deleted), "<<<<<<< HEAD\n=======\nid = \"01jabcdefghjkmnpqrstvwxyz1\"\ntype = \"debug/print\"\nposition = [1.0, 1.0]\n>>>>>>> other\n").unwrap();

        let conflicts = graph_conflicts(&graph).unwrap();
        assert_eq!(conflicts.len(), 2);
        let ConflictSide::Node { node } = &conflicts[0].theirs else {
            panic!("expected a node");
        };
        assert_eq!(node.position, [100.0, 40.0]);
        assert_eq!(conflicts[1].ours, ConflictSide::Deleted);

        resolve_conflict(&graph, file, ConflictChoice::Theirs).unwrap();
        resolve_conflict(&graph, deleted, ConflictChoice::Ours).unwrap();
        assert!(!graph.join(deleted).exists());
        let document = crate::GraphDocument::load(&graph).unwrap();
        assert!(document.load_errors.is_empty());
        assert_eq!(document.nodes[0].position, [100.0, 40.0]);
        assert!(resolve_conflict(&graph, "../escape.toml", ConflictChoice::Ours).is_err());
    }

    #[test]
    fn reads_the_last_committed_nodes() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let run = |arguments: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap().status.success(), "{arguments:?}");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        let graph = create_graph(root, "Main", GraphKind::EventGraph).unwrap();
        let node = NodeDocument::new("debug/print", [0.0, 0.0]);
        node.write(&graph).unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "First graph"]);
        NodeDocument::new("debug/print", [1.0, 1.0]).write(&graph).unwrap();

        let committed = committed_graph(&graph).unwrap();
        assert_eq!(committed.summary, "First graph");
        assert_eq!(committed.nodes, [node]);
        assert!(committed_graph(&tempfile::tempdir().unwrap().path().join("missing")).is_none());
    }
}
