use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::fs_util::sorted_subdirectories;
use crate::tree::{FolderSummary, GraphSummary, migrate_legacy_layout, scan_package_tree};
use crate::{FieldIssue, Id, LocalSettings, PackageManifest, PackageType, ProjectError, ProjectManifest};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageSummary {
    pub id: Id,
    pub folder: String,
    pub path: PathBuf,
    pub title: String,
    pub package_type: PackageType,
    pub version: String,
    pub requirements: Vec<String>,
    pub custom_settings: Vec<crate::CustomSetting>,
    pub graphs: Vec<GraphSummary>,
    pub folders: Vec<FolderSummary>,
}

#[derive(Debug, Clone)]
pub struct Project {
    project_file: PathBuf,
    root: PathBuf,
    manifest: ProjectManifest,
    local: LocalSettings,
    packages: Vec<PackageSummary>,
}

impl Project {
    pub const PACKAGES_DIRECTORY: &'static str = "packages";
    pub const GRAPHS_DIRECTORY: &'static str = "graphs";
    pub const LOCAL_DIRECTORY: &'static str = ".nanosflow";

    pub fn open(project_file: impl AsRef<Path>) -> Result<Project, ProjectError> {
        let project_file = project_file.as_ref().to_path_buf();
        let manifest = ProjectManifest::read(&project_file)?;
        let root = project_file
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let packages = scan_packages(&root.join(Self::PACKAGES_DIRECTORY))?;
        let local = LocalSettings::read(&root)?;
        Ok(Project {
            project_file,
            root,
            manifest,
            local,
            packages,
        })
    }

    pub fn project_file(&self) -> &Path {
        &self.project_file
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &ProjectManifest {
        &self.manifest
    }

    pub fn local_settings(&self) -> &LocalSettings {
        &self.local
    }

    pub fn output_directory(&self) -> PathBuf {
        match &self.local.output_directory {
            Some(directory) => self.root.join(directory),
            None => self.root.join(&self.manifest.build.output_directory),
        }
    }

    pub fn packages(&self) -> &[PackageSummary] {
        &self.packages
    }

    pub fn update_custom_settings(&self, package_directory: &Path, settings: Vec<crate::CustomSetting>) -> Result<(), ProjectError> {
        if !self.packages.iter().any(|package| package.path == package_directory) {
            return Err(ProjectError::InvalidOptions {
                issues: vec![FieldIssue::error("customSettings", "This package is not part of the project.")],
            });
        }
        let issues = crate::validate_custom_settings(&settings);
        if !issues.is_empty() {
            return Err(ProjectError::InvalidOptions { issues });
        }
        let mut manifest = crate::PackageManifest::read(package_directory)?;
        manifest.custom_settings = settings;
        manifest.write(package_directory)
    }

    pub fn update_requirements(&self, package_directory: &Path, requirements: &[String]) -> Result<(), ProjectError> {
        let Some(package) = self.packages.iter().find(|package| package.path == package_directory) else {
            return Err(ProjectError::InvalidOptions {
                issues: vec![FieldIssue::error("requirements", "This package is not part of the project.")],
            });
        };
        let mut cleaned: Vec<String> = Vec::new();
        let mut issues = Vec::new();
        for requirement in requirements {
            let requirement = requirement.trim();
            if requirement.is_empty() || cleaned.iter().any(|existing| existing.eq_ignore_ascii_case(requirement)) {
                continue;
            }
            if requirement.eq_ignore_ascii_case(&package.folder) {
                issues.push(FieldIssue::error("requirements", "A package cannot require itself."));
            } else if !requirement.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')) {
                issues.push(FieldIssue::error(
                    "requirements",
                    format!("'{requirement}' is not a package folder name. Use the folder name from the Packages folder, for example 'default-vehicles'."),
                ));
            } else {
                cleaned.push(requirement.to_owned());
            }
        }
        if let Some(cycle) = self.requirement_cycle(&package.folder, &cleaned) {
            issues.push(FieldIssue::error(
                "requirements",
                format!("This would make the packages require each other in a circle ({}), so none of them could load first.", cycle.join(" -> ")),
            ));
        }
        if !issues.is_empty() {
            return Err(ProjectError::InvalidOptions { issues });
        }
        let mut manifest = PackageManifest::read(package_directory)?;
        manifest.packages_requirements = cleaned;
        manifest.write(package_directory)
    }

    fn requirement_cycle(&self, folder: &str, requirements: &[String]) -> Option<Vec<String>> {
        let requirements_of = |name: &str| -> Vec<String> {
            if name == folder {
                return requirements.to_vec();
            }
            self.packages
                .iter()
                .find(|package| package.folder == name)
                .map(|package| package.requirements.clone())
                .unwrap_or_default()
        };
        let mut path = vec![folder.to_owned()];
        leads_back(folder, folder, &mut path, &requirements_of).then_some(path)
    }
}

fn leads_back(current: &str, start: &str, path: &mut Vec<String>, requirements_of: &dyn Fn(&str) -> Vec<String>) -> bool {
    for next in requirements_of(current) {
        if next == start {
            path.push(next);
            return true;
        }
        if path.contains(&next) {
            continue;
        }
        path.push(next.clone());
        if leads_back(&next, start, path, requirements_of) {
            return true;
        }
        path.pop();
    }
    false
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn scan_packages(packages_directory: &Path) -> Result<Vec<PackageSummary>, ProjectError> {
    let mut packages = Vec::new();
    for directory in sorted_subdirectories(packages_directory)? {
        if !directory.join(PackageManifest::FILE_NAME).is_file() {
            continue;
        }
        let manifest = PackageManifest::read(&directory)?;
        migrate_legacy_layout(&directory)?;
        let tree = scan_package_tree(&directory)?;
        packages.push(PackageSummary {
            id: manifest.id,
            folder: folder_name(&directory),
            path: directory.clone(),
            title: manifest.title,
            package_type: manifest.package_type,
            version: manifest.version,
            requirements: manifest.packages_requirements,
            custom_settings: manifest.custom_settings,
            graphs: tree.graphs,
            folders: tree.folders,
        });
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(root: &Path, folder: &str, requirements: &[&str]) -> PathBuf {
        let directory = root.join(Project::PACKAGES_DIRECTORY).join(folder);
        PackageManifest {
            id: Id::generate(),
            package_type: PackageType::Script,
            title: folder.to_owned(),
            author: String::new(),
            version: "1.0.0".to_owned(),
            compatibility_version: crate::DEFAULT_COMPATIBILITY_VERSION.to_owned(),
            auto_cleanup: false,
            load_level_entities: false,
            packages_requirements: requirements.iter().map(|requirement| (*requirement).to_owned()).collect(),
            assets_requirements: Vec::new(),
            map_asset: None,
            custom_settings: Vec::new(),
        }
        .write(&directory)
        .unwrap();
        directory
    }

    #[test]
    fn updates_requirements_and_refuses_circles() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let project_file = root.join("Test.nfproj");
        ProjectManifest::new("Test", nf_api::ApiChannel::Stable).write(&project_file).unwrap();
        let game = package(root, "game", &[]);
        package(root, "core", &["utils"]);
        package(root, "utils", &[]);
        let project = Project::open(&project_file).unwrap();

        project
            .update_requirements(&game, &[" core ".to_owned(), "core".to_owned(), "default-vehicles".to_owned()])
            .unwrap();
        assert_eq!(PackageManifest::read(&game).unwrap().packages_requirements, ["core", "default-vehicles"]);

        let utils = root.join(Project::PACKAGES_DIRECTORY).join("utils");
        let refused = project.update_requirements(&utils, &["core".to_owned()]);
        let Err(ProjectError::InvalidOptions { issues }) = refused else {
            panic!("a circle must be refused");
        };
        assert!(issues[0].message.contains("utils -> core -> utils"), "{}", issues[0].message);
        assert!(project.update_requirements(&game, &["game".to_owned()]).is_err());
        assert!(project.update_requirements(&game, &["bad name".to_owned()]).is_err());
    }
}
