use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use nf_graph::{DiagnosticSeverity, NodeCatalog};
use nf_project::{Project, ProjectError};
use serde::Serialize;

use crate::package::compile_package;
use crate::symbols::BuildProblem;

pub const BUILD_MANIFEST: &str = ".nanosflow-build";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageReport {
    pub folder: String,
    pub title: String,
    pub output_directory: PathBuf,
    pub written: bool,
    pub file_count: usize,
    pub changed_files: usize,
    pub removed_files: usize,
    pub problems: Vec<BuildProblem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildReport {
    pub output_directory: PathBuf,
    pub packages: Vec<PackageReport>,
    pub error_count: usize,
    pub warning_count: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WriteSummary {
    pub changed: usize,
    pub removed: usize,
}

fn is_safe_relative(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty() && path.components().all(|component| matches!(component, Component::Normal(_)))
}

fn previous_files(directory: &Path) -> BTreeSet<String> {
    fs::read_to_string(directory.join(BUILD_MANIFEST))
        .map(|content| {
            content
                .lines()
                .map(str::trim)
                .filter(|line| is_safe_relative(line))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn remove_empty_parents(directory: &Path, file: &Path) {
    let mut current = file.parent();
    while let Some(parent) = current {
        if parent == directory || !parent.starts_with(directory) {
            break;
        }
        if fs::remove_dir(parent).is_err() {
            break;
        }
        current = parent.parent();
    }
}

pub fn is_foreign_directory(directory: &Path) -> bool {
    let has_content = fs::read_dir(directory).is_ok_and(|mut entries| entries.next().is_some());
    has_content && !directory.join(BUILD_MANIFEST).is_file()
}

pub fn write_output(directory: &Path, files: &BTreeMap<String, String>) -> Result<WriteSummary, ProjectError> {
    let mut summary = WriteSummary::default();
    for (relative, content) in files {
        let path = directory.join(relative);
        if fs::read_to_string(&path).is_ok_and(|existing| existing == *content) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ProjectError::io(parent))?;
        }
        fs::write(&path, content).map_err(ProjectError::io(&path))?;
        summary.changed += 1;
    }
    for stale in previous_files(directory) {
        if files.contains_key(&stale) {
            continue;
        }
        let path = directory.join(&stale);
        if path.is_file() {
            fs::remove_file(&path).map_err(ProjectError::io(&path))?;
            summary.removed += 1;
            remove_empty_parents(directory, &path);
        }
    }
    let listing: String = files.keys().map(|relative| format!("{relative}\n")).collect();
    let manifest = directory.join(BUILD_MANIFEST);
    if fs::read_to_string(&manifest).ok().as_deref() != Some(listing.as_str()) {
        fs::write(&manifest, listing).map_err(ProjectError::io(&manifest))?;
    }
    Ok(summary)
}

pub fn build_project(project: &Project, catalog: &NodeCatalog) -> Result<BuildReport, ProjectError> {
    build_project_into(project, catalog, project.output_directory())
}

pub fn build_project_into(project: &Project, catalog: &NodeCatalog, output_directory: PathBuf) -> Result<BuildReport, ProjectError> {
    let mut packages = Vec::new();
    for package in project.packages() {
        let mut compiled = compile_package(catalog, &package.path)?;
        let directory = output_directory.join(&package.folder);
        if is_foreign_directory(&directory) {
            compiled.problems.insert(
                0,
                BuildProblem {
                    severity: DiagnosticSeverity::Error,
                    code: "output-not-owned".to_owned(),
                    graph: None,
                    node: None,
                    pin: None,
                    message: format!(
                        "The folder '{}' already exists and was not created by NanosFlow, so the build does not touch it.",
                        directory.display()
                    ),
                    hint: Some(
                        "Rename or move that folder, or pick another output folder in the project settings.".to_owned(),
                    ),
                },
            );
        }
        let written = !compiled.has_errors();
        let summary = if written {
            let summary = write_output(&directory, &compiled.files)?;
            crate::debug::save_source_map(project.root(), &package.folder, &compiled.source_map)?;
            summary
        } else {
            WriteSummary::default()
        };
        packages.push(PackageReport {
            folder: package.folder.clone(),
            title: package.title.clone(),
            output_directory: directory,
            written,
            file_count: compiled.files.len(),
            changed_files: summary.changed,
            removed_files: summary.removed,
            problems: compiled.problems,
        });
    }
    let count = |severity: DiagnosticSeverity| {
        packages
            .iter()
            .flat_map(|package| &package.problems)
            .filter(|problem| problem.severity == severity)
            .count()
    };
    Ok(BuildReport {
        error_count: count(DiagnosticSeverity::Error),
        warning_count: count(DiagnosticSeverity::Warning),
        output_directory,
        packages,
    })
}
