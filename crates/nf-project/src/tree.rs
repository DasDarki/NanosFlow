use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fs_util::{read_toml, sorted_subdirectories, write_text};
use crate::validation::FieldIssue;
use crate::{GraphKind, GraphManifest, Id, NodeDocument, PackageManifest, Project, ProjectError, ScriptContext};

const MAX_ENTRY_NAME_LENGTH: usize = 64;
const RESERVED_WINDOWS_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
pub const FOLDER_KEEP_FILE: &str = ".gitkeep";
pub const METHODS_DIRECTORY: &str = "functions";
pub const DISPATCHERS_DIRECTORY: &str = "dispatchers";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryLocation {
    pub package_path: PathBuf,
    pub context: ScriptContext,
    #[serde(default)]
    pub folders: Vec<String>,
}

impl EntryLocation {
    pub fn directory(&self) -> PathBuf {
        let mut directory = context_root(&self.package_path, self.context);
        for folder in &self.folders {
            directory.push(folder);
        }
        directory
    }
}

pub fn context_root(package_directory: &Path, context: ScriptContext) -> PathBuf {
    package_directory
        .join(Project::GRAPHS_DIRECTORY)
        .join(context.directory_name())
}

pub fn validate_entry_name(name: &str) -> Vec<FieldIssue> {
    const FIELD: &str = "name";
    let mut issues = Vec::new();
    if name.trim().is_empty() {
        issues.push(FieldIssue::error(FIELD, "Enter a name."));
        return issues;
    }
    if name.chars().count() > MAX_ENTRY_NAME_LENGTH {
        issues.push(FieldIssue::error(
            FIELD,
            format!("The name can be at most {MAX_ENTRY_NAME_LENGTH} characters long."),
        ));
    }
    let invalid: Vec<char> = name
        .chars()
        .filter(|character| !(character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_')))
        .collect();
    if !invalid.is_empty() {
        let mut listed: Vec<String> = invalid.iter().map(|character| format!("'{character}'")).collect();
        listed.dedup();
        issues.push(FieldIssue::error(
            FIELD,
            format!(
                "The name contains {}. Use letters, digits, spaces, '-' and '_' only, because the name is also a folder and Lua file name.",
                listed.join(", ")
            ),
        ));
    }
    if name.starts_with(' ') || name.ends_with(' ') {
        issues.push(FieldIssue::error(FIELD, "The name must not start or end with a space."));
    }
    if name.starts_with(|character: char| character.is_ascii_digit()) {
        issues.push(FieldIssue::error(
            FIELD,
            "The name must not start with a digit, because it is also used as a Lua identifier.",
        ));
    }
    if RESERVED_WINDOWS_NAMES.iter().any(|reserved| reserved.eq_ignore_ascii_case(name.trim())) {
        issues.push(FieldIssue::error(
            FIELD,
            format!("'{name}' is a reserved name on Windows and cannot be used."),
        ));
    }
    issues
}

pub fn validate_class_name(name: &str) -> Vec<FieldIssue> {
    let mut issues = validate_entry_name(name);
    if !issues.is_empty() {
        return issues;
    }
    let is_identifier = name.starts_with(|character: char| character.is_ascii_alphabetic())
        && name.chars().all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !is_identifier {
        issues.push(FieldIssue::error(
            "name",
            "Class names become Lua identifiers, so they may only contain letters, digits and '_' and must start with a letter. For example 'ZombieCharacter'.",
        ));
    }
    issues
}

fn checked_class_name(name: &str) -> Result<&str, ProjectError> {
    let issues = validate_class_name(name);
    if issues.is_empty() {
        Ok(name)
    } else {
        Err(ProjectError::InvalidOptions { issues })
    }
}

fn entry_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn ensure_free(parent: &Path, name: &str, ignore: Option<&Path>) -> Result<(), ProjectError> {
    let taken = std::fs::read_dir(parent)
        .map(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry.file_name().to_string_lossy().eq_ignore_ascii_case(name)
                    && ignore.is_none_or(|ignored| entry.path() != ignored)
            })
        })
        .unwrap_or(false);
    if taken {
        return Err(ProjectError::InvalidOptions {
            issues: vec![FieldIssue::error(
                "name",
                format!("Something called '{name}' already exists here. Names must be unique, ignoring upper and lower case, so the project also works on Windows."),
            )],
        });
    }
    Ok(())
}

fn checked_name(name: &str) -> Result<&str, ProjectError> {
    let issues = validate_entry_name(name);
    if issues.is_empty() {
        Ok(name)
    } else {
        Err(ProjectError::InvalidOptions { issues })
    }
}

pub fn is_graph_directory(path: &Path) -> bool {
    path.join(GraphManifest::FILE_NAME).is_file()
}

pub fn create_graph(parent: &Path, name: &str, kind: GraphKind) -> Result<PathBuf, ProjectError> {
    let name = checked_name(name)?;
    ensure_free(parent, name, None)?;
    let directory = parent.join(name);
    GraphManifest::new(kind).write(&directory)?;
    if matches!(kind, GraphKind::Event | GraphKind::Struct | GraphKind::Enum | GraphKind::Dispatcher | GraphKind::Interface) {
        return Ok(directory);
    }
    let nodes_directory = directory.join(GraphManifest::NODES_DIRECTORY);
    std::fs::create_dir_all(&nodes_directory).map_err(ProjectError::io(&nodes_directory))?;
    match kind {
        GraphKind::Function => NodeDocument::new("function/entry", [0.0, 0.0]).write(&directory)?,
        GraphKind::Macro => {
            NodeDocument::new("macro/inputs", [0.0, 0.0]).write(&directory)?;
            NodeDocument::new("macro/outputs", [480.0, 0.0]).write(&directory)?;
        }
        _ => {}
    }
    Ok(directory)
}

pub fn create_class(parent: &Path, name: &str, parent_class: &str) -> Result<PathBuf, ProjectError> {
    let name = checked_class_name(name)?;
    ensure_free(parent, name, None)?;
    let directory = parent.join(name);
    let mut manifest = GraphManifest::new(GraphKind::Class);
    manifest.parent = Some(parent_class.to_owned());
    manifest.write(&directory)?;
    let nodes_directory = directory.join(GraphManifest::NODES_DIRECTORY);
    std::fs::create_dir_all(&nodes_directory).map_err(ProjectError::io(&nodes_directory))?;
    Ok(directory)
}

pub fn create_method(class_directory: &Path, name: &str) -> Result<PathBuf, ProjectError> {
    let manifest = GraphManifest::read(class_directory)?;
    if manifest.kind != GraphKind::Class {
        return Err(ProjectError::InvalidOptions {
            issues: vec![FieldIssue::error("name", "Methods can only be added to classes.")],
        });
    }
    let methods = class_directory.join(METHODS_DIRECTORY);
    std::fs::create_dir_all(&methods).map_err(ProjectError::io(&methods))?;
    create_graph(&methods, name, GraphKind::Function)
}

fn comparable(name: &str) -> String {
    name.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

pub fn add_interface_methods(class_directory: &Path, functions: &[crate::InterfaceFunction]) -> Result<Vec<PathBuf>, ProjectError> {
    let existing: Vec<String> = sorted_subdirectories(&class_directory.join(METHODS_DIRECTORY))?
        .iter()
        .map(|method| comparable(&entry_name(method)))
        .collect();
    let mut created = Vec::new();
    for function in functions {
        if existing.contains(&comparable(&function.name)) {
            continue;
        }
        let method = create_method(class_directory, &function.name)?;
        crate::update_function_signature(
            &method,
            crate::FunctionSignature {
                description: function.description.clone(),
                parameters: function.parameters.clone(),
                returns: function.returns.clone(),
                ..crate::FunctionSignature::default()
            },
        )?;
        created.push(method);
    }
    Ok(created)
}

pub fn create_dispatcher(class_directory: &Path, name: &str) -> Result<PathBuf, ProjectError> {
    let manifest = GraphManifest::read(class_directory)?;
    if manifest.kind != GraphKind::Class {
        return Err(ProjectError::InvalidOptions {
            issues: vec![FieldIssue::error("name", "Event dispatchers can only be added to classes.")],
        });
    }
    let dispatchers = class_directory.join(DISPATCHERS_DIRECTORY);
    std::fs::create_dir_all(&dispatchers).map_err(ProjectError::io(&dispatchers))?;
    if sorted_subdirectories(&class_directory.join(METHODS_DIRECTORY))?
        .iter()
        .any(|method| entry_name(method).eq_ignore_ascii_case(name.trim()))
    {
        return Err(ProjectError::InvalidOptions {
            issues: vec![FieldIssue::error("name", format!("This class already has a method called '{}'.", name.trim()))],
        });
    }
    create_graph(&dispatchers, name, GraphKind::Dispatcher)
}

pub fn class_names(package_directory: &Path) -> Result<Vec<String>, ProjectError> {
    Ok(scan_package_tree(package_directory)?
        .graphs
        .into_iter()
        .filter(|graph| graph.kind == GraphKind::Class)
        .map(|graph| graph.name)
        .collect())
}

pub fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, ProjectError> {
    let name = checked_name(name)?;
    ensure_free(parent, name, None)?;
    let directory = parent.join(name);
    write_text(&directory.join(FOLDER_KEEP_FILE), "")?;
    Ok(directory)
}

pub fn rename_entry(path: &Path, new_name: &str) -> Result<PathBuf, ProjectError> {
    let is_class = is_graph_directory(path) && matches!(GraphManifest::read(path)?.kind, GraphKind::Class | GraphKind::Struct | GraphKind::Enum | GraphKind::Interface);
    let new_name = if is_class { checked_class_name(new_name)? } else { checked_name(new_name)? };
    let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
    ensure_free(&parent, new_name, Some(path))?;
    let target = parent.join(new_name);
    let old_name = entry_name(path);
    std::fs::rename(path, &target).map_err(ProjectError::io(path))?;
    if is_class
        && let Some(package) = target.ancestors().find(|ancestor| ancestor.join(PackageManifest::FILE_NAME).is_file())
    {
        crate::rename_type_references(package, &old_name, new_name)?;
    }
    Ok(target)
}

pub fn move_entry(path: &Path, target_parent: &Path) -> Result<PathBuf, ProjectError> {
    if target_parent.starts_with(path) {
        return Err(ProjectError::InvalidOptions {
            issues: vec![FieldIssue::error("name", "A folder cannot be moved into itself.")],
        });
    }
    let name = entry_name(path);
    let target = target_parent.join(&name);
    if target == path {
        return Ok(target);
    }
    ensure_free(target_parent, &name, None)?;
    std::fs::create_dir_all(target_parent).map_err(ProjectError::io(target_parent))?;
    std::fs::rename(path, &target).map_err(ProjectError::io(path))?;
    remove_keep_file_if_unneeded(target_parent);
    Ok(target)
}

fn remove_keep_file_if_unneeded(folder: &Path) {
    let keep = folder.join(FOLDER_KEEP_FILE);
    if keep.is_file() && !sorted_subdirectories(folder).unwrap_or_default().is_empty() {
        let _ = std::fs::remove_file(keep);
    }
}

pub fn delete_entry(path: &Path) -> Result<(), ProjectError> {
    let parent = path.parent().map(Path::to_path_buf);
    std::fs::remove_dir_all(path).map_err(ProjectError::io(path))?;
    if let Some(parent) = parent {
        let is_empty_folder = std::fs::read_dir(&parent).is_ok_and(|mut entries| entries.next().is_none());
        if is_empty_folder && !is_context_root(&parent) {
            write_text(&parent.join(FOLDER_KEEP_FILE), "")?;
        }
    }
    Ok(())
}

fn is_context_root(path: &Path) -> bool {
    let is_context_name = path
        .file_name()
        .is_some_and(|name| ScriptContext::ALL.iter().any(|context| name == context.directory_name()));
    let parent_is_graphs = path
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == Project::GRAPHS_DIRECTORY);
    is_context_name && parent_is_graphs
}

#[derive(Deserialize)]
struct LegacyGraphManifest {
    context: ScriptContext,
    #[serde(default)]
    name: Option<String>,
}

pub fn migrate_legacy_layout(package_directory: &Path) -> Result<(), ProjectError> {
    let graphs_directory = package_directory.join(Project::GRAPHS_DIRECTORY);
    for directory in sorted_subdirectories(&graphs_directory)? {
        if !is_graph_directory(&directory) {
            continue;
        }
        let legacy: LegacyGraphManifest = read_toml(&directory.join(GraphManifest::FILE_NAME))?;
        let root = context_root(package_directory, legacy.context);
        std::fs::create_dir_all(&root).map_err(ProjectError::io(&root))?;
        let base = legacy
            .name
            .filter(|name| validate_entry_name(name).is_empty())
            .unwrap_or_else(|| entry_name(&directory));
        let mut name = base.clone();
        let mut suffix = 2;
        while root.join(&name).exists() {
            name = format!("{base} {suffix}");
            suffix += 1;
        }
        std::fs::rename(&directory, root.join(&name)).map_err(ProjectError::io(&directory))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphSummary {
    pub id: Id,
    pub name: String,
    pub path: PathBuf,
    pub kind: GraphKind,
    pub context: ScriptContext,
    pub folders: Vec<String>,
    pub node_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub exported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSummary {
    pub name: String,
    pub path: PathBuf,
    pub context: ScriptContext,
    pub folders: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PackageTree {
    pub graphs: Vec<GraphSummary>,
    pub folders: Vec<FolderSummary>,
}

fn count_nodes(directory: &Path) -> usize {
    std::fs::read_dir(directory.join(GraphManifest::NODES_DIRECTORY))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|extension| extension == NodeDocument::EXTENSION))
                .count()
        })
        .unwrap_or(0)
}

fn scan_directory(
    directory: &Path,
    context: ScriptContext,
    folders: &mut Vec<String>,
    tree: &mut PackageTree,
) -> Result<(), ProjectError> {
    for child in sorted_subdirectories(directory)? {
        let name = entry_name(&child);
        if name.starts_with('.') {
            continue;
        }
        if is_graph_directory(&child) {
            let manifest = GraphManifest::read(&child)?;
            if manifest.kind == GraphKind::Class {
                let members = sorted_subdirectories(&child.join(METHODS_DIRECTORY))?
                    .into_iter()
                    .chain(sorted_subdirectories(&child.join(DISPATCHERS_DIRECTORY))?);
                for method in members {
                    if !is_graph_directory(&method) {
                        continue;
                    }
                    let method_manifest = GraphManifest::read(&method)?;
                    tree.graphs.push(GraphSummary {
                        id: method_manifest.id,
                        name: entry_name(&method),
                        node_count: count_nodes(&method),
                        path: method,
                        kind: method_manifest.kind,
                        context,
                        folders: folders.clone(),
                        owner: Some(manifest.id),
                        parent: None,
                        exported: false,
                    });
                }
            }
            tree.graphs.push(GraphSummary {
                id: manifest.id,
                name,
                node_count: count_nodes(&child),
                path: child,
                kind: manifest.kind,
                context,
                folders: folders.clone(),
                owner: None,
                exported: matches!(manifest.kind, GraphKind::Function | GraphKind::Class) && manifest.signature.exported,
                parent: manifest.parent,
            });
        } else {
            tree.folders.push(FolderSummary {
                name: name.clone(),
                path: child.clone(),
                context,
                folders: folders.clone(),
            });
            folders.push(name);
            scan_directory(&child, context, folders, tree)?;
            folders.pop();
        }
    }
    Ok(())
}

pub fn scan_package_tree(package_directory: &Path) -> Result<PackageTree, ProjectError> {
    let mut tree = PackageTree::default();
    for context in ScriptContext::ALL {
        scan_directory(&context_root(package_directory, context), context, &mut Vec::new(), &mut tree)?;
    }
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(package: &Path, context: ScriptContext, folders: &[&str]) -> PathBuf {
        EntryLocation {
            package_path: package.to_path_buf(),
            context,
            folders: folders.iter().map(|folder| (*folder).to_owned()).collect(),
        }
        .directory()
    }

    #[test]
    fn builds_a_tree_of_folders_and_graphs() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let server = context_root(package, ScriptContext::Server);
        let weapons = create_folder(&server, "Weapons").unwrap();
        create_graph(&weapons, "Rifle", GraphKind::EventGraph).unwrap();
        create_graph(&location(package, ScriptContext::Shared, &[]), "Math Helpers", GraphKind::Function).unwrap();

        let tree = scan_package_tree(package).unwrap();
        assert_eq!(tree.folders.len(), 1);
        let rifle = tree.graphs.iter().find(|graph| graph.name == "Rifle").unwrap();
        assert_eq!(rifle.context, ScriptContext::Server);
        assert_eq!(rifle.folders, ["Weapons"]);
        let helpers = tree.graphs.iter().find(|graph| graph.name == "Math Helpers").unwrap();
        assert_eq!(helpers.kind, GraphKind::Function);
        assert_eq!(helpers.node_count, 1);
    }

    #[test]
    fn rejects_case_insensitive_duplicates_and_bad_names() {
        let temporary = tempfile::tempdir().unwrap();
        let root = context_root(temporary.path(), ScriptContext::Client);
        create_graph(&root, "Hud", GraphKind::EventGraph).unwrap();
        assert!(create_graph(&root, "HUD", GraphKind::EventGraph).is_err());
        assert!(!validate_entry_name("a/b").is_empty());
        assert!(!validate_entry_name("1st").is_empty());
        assert!(!validate_entry_name("con").is_empty());
        assert!(validate_entry_name("Round Logic_2").is_empty());
    }

    #[test]
    fn moves_and_renames_entries() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let server = context_root(package, ScriptContext::Server);
        let folder = create_folder(&server, "Combat").unwrap();
        let graph = create_graph(&server, "Damage", GraphKind::EventGraph).unwrap();

        let moved = move_entry(&graph, &folder).unwrap();
        assert!(moved.join(GraphManifest::FILE_NAME).is_file());
        assert!(!folder.join(FOLDER_KEEP_FILE).exists());
        assert!(move_entry(&folder, &folder.join("Nested")).is_err());

        let renamed = rename_entry(&moved, "Damage Rules").unwrap();
        assert!(renamed.ends_with("Damage Rules"));
        let client = context_root(package, ScriptContext::Client);
        let across = move_entry(&renamed, &client).unwrap();
        let tree = scan_package_tree(package).unwrap();
        assert_eq!(tree.graphs[0].context, ScriptContext::Client);
        delete_entry(&across).unwrap();
        assert!(scan_package_tree(package).unwrap().graphs.is_empty());
    }

    #[test]
    fn classes_own_their_methods() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let shared = context_root(package, ScriptContext::Shared);
        assert!(create_class(&shared, "Zombie Character", "Character").is_err());
        let class = create_class(&shared, "ZombieCharacter", "Character").unwrap();
        create_method(&class, "Bite").unwrap();
        assert!(create_method(&create_graph(&shared, "Main", GraphKind::EventGraph).unwrap(), "Nope").is_err());

        let tree = scan_package_tree(package).unwrap();
        let class_summary = tree.graphs.iter().find(|graph| graph.kind == GraphKind::Class).unwrap();
        assert_eq!(class_summary.parent.as_deref(), Some("Character"));
        let bite = tree.graphs.iter().find(|graph| graph.name == "Bite").unwrap();
        assert_eq!(bite.owner, Some(class_summary.id));
        assert!(rename_entry(&class, "Bad Name").is_err());
        assert_eq!(class_names(package).unwrap(), ["ZombieCharacter"]);
    }

    #[test]
    fn migrates_graphs_from_the_flat_layout() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let legacy = package.join(Project::GRAPHS_DIRECTORY).join("main-server");
        std::fs::create_dir_all(legacy.join("nodes")).unwrap();
        std::fs::write(
            legacy.join(GraphManifest::FILE_NAME),
            "id = \"01jabcdefghjkmnpqrstvwxyz0\"\nname = \"Main\"\nkind = \"event-graph\"\ncontext = \"server\"\n",
        )
        .unwrap();

        migrate_legacy_layout(package).unwrap();
        let tree = scan_package_tree(package).unwrap();
        assert_eq!(tree.graphs.len(), 1);
        assert_eq!(tree.graphs[0].name, "Main");
        assert_eq!(tree.graphs[0].context, ScriptContext::Server);
    }
}
