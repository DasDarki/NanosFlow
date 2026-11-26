use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("could not access '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("'{path}' contains invalid data: {source}")]
    Deserialize {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("could not serialize '{path}': {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },

    #[error("'{path}' is not a NanosFlow project file (expected the extension .{expected})")]
    NotAProjectFile { path: PathBuf, expected: &'static str },

    #[error(
        "'{path}' was created by a newer NanosFlow (format {found}, this version supports up to {supported}). Please update NanosFlow."
    )]
    UnsupportedFormat {
        path: PathBuf,
        found: u32,
        supported: u32,
    },

    #[error("the project settings are invalid: {}", .issues.iter().map(|issue| issue.message.as_str()).collect::<Vec<_>>().join("; "))]
    InvalidOptions { issues: Vec<crate::FieldIssue> },
}

impl ProjectError {
    pub fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> ProjectError {
        let path = path.into();
        move |source| ProjectError::Io { path, source }
    }
}
