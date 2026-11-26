mod error;
mod fs_util;
mod graph;
mod id;
mod local;
mod manifest;
mod merge;
mod node;
mod package;
mod project;
mod refactor;
mod scaffold;
mod tree;
mod validation;
mod variable;

pub use error::ProjectError;
pub use graph::{
    ClassSettings, EventDirection, FunctionPin, FunctionSignature, GraphDocument, GraphKind, GraphManifest, InterfaceFunction, NodeLoadError, ScriptContext,
    apply_graph_changes, apply_variable_changes, load_variables, update_class, update_event, update_function_signature, update_interface,
};
pub use variable::VariableDocument;
pub use id::Id;
pub use local::{DEFAULT_SERVER_PORT, LocalSettings, ServerBranch, SteamApp, TestServerSettings};
pub use manifest::{BuildSettings, FORMAT_VERSION, PROJECT_EXTENSION, ProjectManifest};
pub use merge::{
    CommittedGraph, ConflictChoice, ConflictKind, ConflictSide, ConflictSides, FileConflict, committed_graph, graph_conflicts, has_conflict_markers,
    resolve_conflict, split_conflict,
};
pub use node::{NodeDocument, PinLink, PinValue};
pub use package::{DEFAULT_COMPATIBILITY_VERSION, PackageManifest, PackageType};
pub use project::{PackageSummary, Project};
pub use refactor::{rename_type_in_expression, rename_type_references};
pub use tree::{
    DISPATCHERS_DIRECTORY, EntryLocation, FolderSummary, GraphSummary, METHODS_DIRECTORY, PackageTree, class_names, context_root, create_class,
    add_interface_methods, create_dispatcher, create_folder, create_graph, create_method, delete_entry, is_graph_directory, move_entry, rename_entry,
    scan_package_tree, validate_class_name, validate_entry_name,
};
pub use scaffold::{NewProjectOptions, ProjectTemplate, create_project};
pub use validation::{FieldIssue, IssueSeverity, slugify, validate_new_project, validate_package_id};
