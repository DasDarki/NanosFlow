use std::fs;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ProjectError;

pub(crate) fn write_text(path: &Path, content: &str) -> Result<(), ProjectError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(ProjectError::io(parent))?;
    }
    let mut normalized = content.replace("\r\n", "\n");
    if !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    if fs::read_to_string(path).is_ok_and(|existing| existing == normalized) {
        return Ok(());
    }
    let temporary = path.with_extension("nftmp");
    fs::write(&temporary, normalized).map_err(ProjectError::io(&temporary))?;
    fs::rename(&temporary, path).map_err(ProjectError::io(path))
}

pub(crate) fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<(), ProjectError> {
    let content = toml::to_string(value).map_err(|source| ProjectError::Serialize {
        path: path.to_path_buf(),
        source,
    })?;
    write_text(path, &content)
}

pub(crate) fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T, ProjectError> {
    let content = fs::read_to_string(path).map_err(ProjectError::io(path))?;
    toml::from_str(&content).map_err(|source| ProjectError::Deserialize {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn sorted_files(path: &Path, extension: &str) -> Result<Vec<std::path::PathBuf>, ProjectError> {
    if !path.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = fs::read_dir(path)
        .map_err(ProjectError::io(path))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry| entry.is_file() && entry.extension().is_some_and(|found| found == extension))
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

pub(crate) fn sorted_subdirectories(path: &Path) -> Result<Vec<std::path::PathBuf>, ProjectError> {
    if !path.is_dir() {
        return Ok(Vec::new());
    }
    let mut directories = fs::read_dir(path)
        .map_err(ProjectError::io(path))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry| entry.is_dir())
        .collect::<Vec<_>>();
    directories.sort();
    Ok(directories)
}
