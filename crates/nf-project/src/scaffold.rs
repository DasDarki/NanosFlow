use std::path::{Path, PathBuf};
use std::process::Command;

use nf_api::ApiChannel;
use serde::{Deserialize, Serialize};

use crate::fs_util::write_text;
use crate::validation::{IssueSeverity, validate_new_project};
use crate::{
    GraphKind, Id, NodeDocument, PROJECT_EXTENSION, PackageManifest, PackageType, Project, ProjectError,
    ProjectManifest, ScriptContext, context_root, create_graph,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectTemplate {
    Blank,
    GameMode,
    Script,
    Map,
}

impl ProjectTemplate {
    pub fn package_type(self) -> PackageType {
        match self {
            ProjectTemplate::Blank | ProjectTemplate::Script => PackageType::Script,
            ProjectTemplate::GameMode => PackageType::GameMode,
            ProjectTemplate::Map => PackageType::Map,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProjectOptions {
    pub name: String,
    pub location: PathBuf,
    pub folder_name: String,
    pub template: ProjectTemplate,
    pub package_id: String,
    pub package_type: PackageType,
    pub author: String,
    pub version: String,
    pub compatibility_version: String,
    pub contexts: Vec<ScriptContext>,
    pub api_channel: ApiChannel,
    pub map_asset: Option<String>,
    pub initialize_git: bool,
}

impl NewProjectOptions {
    pub fn project_directory(&self) -> PathBuf {
        self.location.join(self.folder_name.trim())
    }

    pub fn project_file(&self) -> PathBuf {
        self.project_directory()
            .join(format!("{}.{PROJECT_EXTENSION}", self.folder_name.trim()))
    }
}

const GITIGNORE: &str = "\
/.nanosflow/
/build/
";

const GITATTRIBUTES: &str = "\
* text=auto eol=lf
*.nfproj text eol=lf
*.toml text eol=lf
*.png binary
*.jpg binary
*.webp binary
";

pub fn create_project(options: &NewProjectOptions) -> Result<(Project, Vec<String>), ProjectError> {
    let issues = validate_new_project(options);
    if issues.iter().any(|issue| issue.severity == IssueSeverity::Error) {
        return Err(ProjectError::InvalidOptions { issues });
    }

    let root = options.project_directory();
    std::fs::create_dir_all(&root).map_err(ProjectError::io(&root))?;

    let manifest = ProjectManifest::new(options.name.trim(), options.api_channel);
    let project_file = options.project_file();
    manifest.write(&project_file)?;

    write_text(&root.join(".gitignore"), GITIGNORE)?;
    write_text(&root.join(".gitattributes"), GITATTRIBUTES)?;

    let package_directory = root.join(Project::PACKAGES_DIRECTORY).join(&options.package_id);
    let package = PackageManifest {
        id: Id::generate(),
        package_type: options.package_type,
        title: options.name.trim().to_owned(),
        author: options.author.trim().to_owned(),
        version: options.version.trim().to_owned(),
        compatibility_version: options.compatibility_version.trim().to_owned(),
        auto_cleanup: true,
        load_level_entities: false,
        packages_requirements: Vec::new(),
        assets_requirements: Vec::new(),
        map_asset: options
            .map_asset
            .as_deref()
            .map(str::trim)
            .filter(|asset| !asset.is_empty() && options.package_type == PackageType::Map)
            .map(str::to_owned),
    };
    package.write(&package_directory)?;

    for context in &options.contexts {
        create_starter_graph(&package_directory, *context, options.template)?;
    }

    let mut warnings = Vec::new();
    if options.initialize_git
        && let Err(message) = initialize_git(&root)
    {
        warnings.push(message);
    }

    let project = Project::open(&project_file)?;
    Ok((project, warnings))
}

fn create_starter_graph(
    package_directory: &Path,
    context: ScriptContext,
    template: ProjectTemplate,
) -> Result<(), ProjectError> {
    let graph_directory = create_graph(&context_root(package_directory, context), "Main", GraphKind::EventGraph)?;

    let starter_nodes: &[(&str, [f64; 2])] = match (template, context) {
        (ProjectTemplate::Blank, _) => &[],
        (ProjectTemplate::GameMode, ScriptContext::Server) => &[
            ("event/Package/Load", [0.0, 0.0]),
            ("event/Player/Ready", [0.0, 192.0]),
            ("event/Package/Unload", [0.0, 384.0]),
        ],
        (_, ScriptContext::Shared) => &[],
        _ => &[("event/Package/Load", [0.0, 0.0])],
    };
    for (node_type, position) in starter_nodes {
        NodeDocument::new(*node_type, *position).write(&graph_directory)?;
    }
    Ok(())
}

fn initialize_git(root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(root)
        .output()
        .map_err(|error| {
            format!("Git repository was not created because git could not be started ({error}). Install git and run 'git init' in the project folder.")
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Git repository was not created: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(location: &Path) -> NewProjectOptions {
        NewProjectOptions {
            name: "My Deathmatch".to_owned(),
            location: location.to_path_buf(),
            folder_name: "MyDeathmatch".to_owned(),
            template: ProjectTemplate::GameMode,
            package_id: "my-deathmatch".to_owned(),
            package_type: PackageType::GameMode,
            author: "Tester".to_owned(),
            version: "0.1.0".to_owned(),
            compatibility_version: "1.144".to_owned(),
            contexts: vec![ScriptContext::Server, ScriptContext::Client],
            api_channel: ApiChannel::Stable,
            map_asset: None,
            initialize_git: false,
        }
    }

    #[test]
    fn creates_and_reopens_a_project() {
        let temporary = tempfile::tempdir().unwrap();
        let (project, warnings) = create_project(&options(temporary.path())).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(project.manifest().name, "My Deathmatch");

        let packages = project.packages();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].folder, "my-deathmatch");
        assert_eq!(packages[0].graphs.len(), 2);

        let server = packages[0]
            .graphs
            .iter()
            .find(|graph| graph.context == ScriptContext::Server)
            .unwrap();
        assert_eq!(server.node_count, 3);

        let reopened = Project::open(project.project_file()).unwrap();
        assert_eq!(reopened.manifest().id, project.manifest().id);
    }

    #[test]
    fn refuses_to_overwrite_existing_folders() {
        let temporary = tempfile::tempdir().unwrap();
        let options = options(temporary.path());
        std::fs::create_dir_all(options.project_directory()).unwrap();
        std::fs::write(options.project_directory().join("keep.txt"), "important").unwrap();

        let error = create_project(&options).unwrap_err();
        assert!(matches!(error, ProjectError::InvalidOptions { .. }));
    }

    #[test]
    fn written_files_use_lf_line_endings() {
        let temporary = tempfile::tempdir().unwrap();
        let (project, _) = create_project(&options(temporary.path())).unwrap();
        let content = std::fs::read_to_string(project.project_file()).unwrap();
        assert!(!content.contains('\r'));
    }
}
