use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub title: String,
    pub message: String,
    pub issues: Vec<nf_project::FieldIssue>,
}

impl CommandError {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> CommandError {
        CommandError {
            title: title.into(),
            message: message.into(),
            issues: Vec::new(),
        }
    }
}

impl From<nf_project::ProjectError> for CommandError {
    fn from(error: nf_project::ProjectError) -> CommandError {
        use nf_project::ProjectError;

        let title = match &error {
            ProjectError::NotAProjectFile { .. } => "Not a NanosFlow project",
            ProjectError::UnsupportedFormat { .. } => "Project needs a newer NanosFlow",
            ProjectError::Deserialize { .. } => "Project file is damaged",
            ProjectError::InvalidOptions { .. } => "Project settings need attention",
            ProjectError::Io { .. } | ProjectError::Serialize { .. } => "File access failed",
        };
        let issues = match &error {
            ProjectError::InvalidOptions { issues } => issues.clone(),
            _ => Vec::new(),
        };
        CommandError {
            title: title.to_owned(),
            message: error.to_string(),
            issues,
        }
    }
}

pub type CommandResult<T> = Result<T, CommandError>;
