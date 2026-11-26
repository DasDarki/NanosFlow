use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const REPOSITORY: &str = "https://github.com/nanos-world/api";
pub const BRANCH: &str = "main";
pub const REVISION_FILE: &str = "REVISION";

const KEPT: [&str; 7] = [
    "APIFiles.json",
    "Enums.json",
    "Classes",
    "StaticClasses",
    "StandardLibraries",
    "Structs",
    "UtilityClasses",
];
const STABLE_FOLDER: &str = "Stable";
const TIMEOUT: Duration = Duration::from_secs(60);

fn other(message: impl Into<String>) -> io::Error {
    io::Error::other(message.into())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent("NanosFlow")
        .build()
        .into()
}

pub fn latest_revision() -> io::Result<String> {
    let url = format!("{REPOSITORY}.git/info/refs?service=git-upload-pack");
    let mut response = agent().get(&url).call().map_err(|error| other(format!("{url}: {error}")))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|error| other(format!("{url}: {error}")))?;
    let wanted = format!("refs/heads/{BRANCH}");
    body.lines()
        .filter_map(|line| line.get(4..))
        .find_map(|line| {
            let (sha, reference) = line.split_once(' ')?;
            (reference.trim_end_matches('\0').trim() == wanted && sha.len() == 40).then(|| sha.to_owned())
        })
        .ok_or_else(|| other(format!("{REPOSITORY} did not report the branch {BRANCH}")))
}

pub fn download(revision: &str, destination: &Path) -> io::Result<()> {
    if !revision.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(other(format!("'{revision}' is not a commit id")));
    }
    let url = format!("https://codeload.github.com/nanos-world/api/tar.gz/{revision}");
    let response = agent().get(&url).call().map_err(|error| other(format!("{url}: {error}")))?;
    extract(response.into_body().into_reader(), destination)?;
    std::fs::write(destination.join(REVISION_FILE), format!("{revision}\n"))
}

fn target_of(path: &Path) -> Option<PathBuf> {
    let mut parts = Vec::new();
    for component in path.components().skip(1) {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            _ => return None,
        }
    }
    let (channel, rest) = match parts.first().map(String::as_str) {
        Some(STABLE_FOLDER) => ("stable", &parts[1..]),
        Some(_) => ("latest", &parts[..]),
        None => return None,
    };
    let top = rest.first()?;
    if !KEPT.contains(&top.as_str()) {
        return None;
    }
    let mut target = PathBuf::from(channel);
    target.extend(rest);
    Some(target)
}

pub fn extract(reader: impl Read, destination: &Path) -> io::Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    let mut written = 0usize;
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path()?.into_owned();
        let Some(target) = target_of(&path) else {
            continue;
        };
        let target = destination.join(target);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;
        std::fs::write(&target, content)?;
        written += 1;
    }
    let stable_index = destination.join("stable").join("APIFiles.json");
    if !stable_index.is_file() && destination.join("stable").is_dir() {
        std::fs::copy(destination.join("latest").join("APIFiles.json"), &stable_index)?;
    }
    for channel in ["latest", "stable"] {
        if !destination.join(channel).join("APIFiles.json").is_file() {
            return Err(other(format!("the downloaded API has no {channel} definitions")));
        }
    }
    if written == 0 {
        return Err(other("the downloaded API archive was empty"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_archive_paths_to_channels() {
        assert_eq!(target_of(Path::new("api-main/Classes/Character.json")), Some(PathBuf::from("latest/Classes/Character.json")));
        assert_eq!(target_of(Path::new("api-main/Stable/Enums.json")), Some(PathBuf::from("stable/Enums.json")));
        assert_eq!(target_of(Path::new("api-main/APIFiles.json")), Some(PathBuf::from("latest/APIFiles.json")));
        assert_eq!(target_of(Path::new("api-main/README.md")), None);
        assert_eq!(target_of(Path::new("api-main/Stable/Preprocessor.js")), None);
        assert_eq!(target_of(Path::new("api-main/../Classes/x.json")), None);
    }

    #[test]
    #[ignore = "downloads from GitHub"]
    fn downloads_a_loadable_snapshot() {
        let directory = std::env::temp_dir().join(format!("nanosflow-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let revision = latest_revision().unwrap();
        download(&revision, &directory).unwrap();
        for channel in crate::ApiChannel::ALL {
            let database = crate::ApiDatabase::from_snapshot(&directory, channel).unwrap();
            assert_eq!(database.revision(), revision);
            assert!(database.class("Character").is_some());
        }
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
