use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("API file '{path}' could not be read: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("API file '{path}' is not valid: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("API file '{path}' is missing from the bundled definitions")]
    MissingBundledFile { path: PathBuf },

    #[error("class '{class}' inherits from unknown class '{parent}'")]
    UnknownParent { class: String, parent: String },
}
