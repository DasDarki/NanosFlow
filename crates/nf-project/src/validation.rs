use serde::{Deserialize, Serialize};

use crate::{NewProjectOptions, PackageType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IssueSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldIssue {
    pub field: String,
    pub severity: IssueSeverity,
    pub message: String,
}

impl FieldIssue {
    pub fn error(field: &str, message: impl Into<String>) -> FieldIssue {
        FieldIssue {
            field: field.to_owned(),
            severity: IssueSeverity::Error,
            message: message.into(),
        }
    }

    pub fn warning(field: &str, message: impl Into<String>) -> FieldIssue {
        FieldIssue {
            field: field.to_owned(),
            severity: IssueSeverity::Warning,
            message: message.into(),
        }
    }
}

const MAX_NAME_LENGTH: usize = 64;
const RESERVED_WINDOWS_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn slugify(value: &str) -> String {
    let mut slug = String::with_capacity(value.len());
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug.truncate(MAX_NAME_LENGTH);
    slug
}

pub fn validate_package_id(package_id: &str) -> Vec<FieldIssue> {
    const FIELD: &str = "packageId";
    let mut issues = Vec::new();

    if package_id.is_empty() {
        issues.push(FieldIssue::error(FIELD, "The package id is required."));
        return issues;
    }
    if package_id.len() > MAX_NAME_LENGTH {
        issues.push(FieldIssue::error(
            FIELD,
            format!(
                "The package id is {} characters long, nanos world allows at most {MAX_NAME_LENGTH}.",
                package_id.len()
            ),
        ));
    }

    let invalid: Vec<char> = package_id
        .chars()
        .filter(|character| !matches!(character, 'a'..='z' | '0'..='9' | '-'))
        .collect();
    if !invalid.is_empty() {
        let has_uppercase = invalid.iter().any(char::is_ascii_uppercase);
        let listed = invalid
            .iter()
            .map(|character| format!("'{character}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let hint = if has_uppercase {
            " Use lowercase letters only."
        } else {
            ""
        };
        issues.push(FieldIssue::error(
            FIELD,
            format!(
                "The package id contains {listed}. nanos world only allows lowercase letters, digits and '-', because the id is also the package folder name.{hint}"
            ),
        ));
    }

    if package_id.starts_with('-') || package_id.ends_with('-') {
        issues.push(FieldIssue::warning(
            FIELD,
            "Package ids usually do not start or end with '-'.",
        ));
    }
    issues
}

fn validate_folder_name(folder_name: &str, issues: &mut Vec<FieldIssue>) {
    const FIELD: &str = "folderName";
    if folder_name.trim().is_empty() {
        issues.push(FieldIssue::error(FIELD, "The folder name is required."));
        return;
    }

    let forbidden: Vec<char> = folder_name
        .chars()
        .filter(|character| {
            matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || character.is_control()
        })
        .collect();
    if !forbidden.is_empty() {
        let listed = forbidden.iter().collect::<String>();
        issues.push(FieldIssue::error(
            FIELD,
            format!("The folder name contains characters that are not allowed on Windows or Linux: {listed}"),
        ));
    }

    let stem = folder_name.split('.').next().unwrap_or_default();
    if RESERVED_WINDOWS_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
    {
        issues.push(FieldIssue::error(
            FIELD,
            format!("'{folder_name}' is a reserved name on Windows and cannot be used as a folder."),
        ));
    }

    if folder_name.ends_with('.') || folder_name.ends_with(' ') {
        issues.push(FieldIssue::error(
            FIELD,
            "The folder name must not end with a dot or a space (Windows would silently strip it).",
        ));
    }
}

fn is_semantic_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|character| character.is_ascii_digit()))
}

fn is_compatibility_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 2
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|character| character.is_ascii_digit()))
}

pub fn validate_new_project(options: &NewProjectOptions) -> Vec<FieldIssue> {
    let mut issues = Vec::new();

    let name = options.name.trim();
    if name.is_empty() {
        issues.push(FieldIssue::error("name", "Give your project a name."));
    } else if name.chars().count() > MAX_NAME_LENGTH {
        issues.push(FieldIssue::error(
            "name",
            format!("The project name can be at most {MAX_NAME_LENGTH} characters long."),
        ));
    }

    validate_folder_name(&options.folder_name, &mut issues);

    if options.location.as_os_str().is_empty() {
        issues.push(FieldIssue::error("location", "Choose where the project should be created."));
    } else if !options.location.is_dir() {
        issues.push(FieldIssue::error(
            "location",
            format!("The folder '{}' does not exist.", options.location.display()),
        ));
    } else if !options.folder_name.trim().is_empty() {
        let target = options.project_directory();
        let is_occupied = target
            .read_dir()
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if is_occupied {
            issues.push(FieldIssue::error(
                "folderName",
                format!(
                    "'{}' already exists and is not empty. Pick another folder name so no files get overwritten.",
                    target.display()
                ),
            ));
        } else if target.is_file() {
            issues.push(FieldIssue::error(
                "folderName",
                format!("'{}' is a file, not a folder.", target.display()),
            ));
        }
    }

    issues.extend(validate_package_id(&options.package_id));

    if options.author.trim().is_empty() {
        issues.push(FieldIssue::warning(
            "author",
            "No author set. The Vault will show the package without an author.",
        ));
    }

    if !is_semantic_version(options.version.trim()) {
        issues.push(FieldIssue::error(
            "version",
            format!(
                "'{}' is not a valid version. Use the format MAJOR.MINOR.PATCH, for example 0.1.0.",
                options.version
            ),
        ));
    }

    if !is_compatibility_version(options.compatibility_version.trim()) {
        issues.push(FieldIssue::error(
            "compatibilityVersion",
            format!(
                "'{}' is not a valid compatibility version. Use the format MAJOR.MINOR, for example 1.144.",
                options.compatibility_version
            ),
        ));
    }

    if options.contexts.is_empty() {
        issues.push(FieldIssue::error(
            "contexts",
            "Select at least one of Server, Client or Shared, otherwise the package has no scripts to run.",
        ));
    }

    if options.package_type == PackageType::Map {
        let map_asset = options.map_asset.as_deref().unwrap_or_default().trim();
        if map_asset.is_empty() {
            issues.push(FieldIssue::error(
                "mapAsset",
                "Map packages need a map asset, for example nanos-world::BlankMap.",
            ));
        } else if map_asset.split("::").filter(|part| !part.is_empty()).count() != 2 {
            issues.push(FieldIssue::error(
                "mapAsset",
                format!("'{map_asset}' is not a valid asset path. Use the format ASSET_PACK::ASSET_KEY."),
            ));
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_produces_valid_package_ids() {
        assert_eq!(slugify("My Awesome Game Mode!"), "my-awesome-game-mode");
        assert_eq!(slugify("  --Ünïcode  Stuff-- "), "n-code-stuff");
        assert!(validate_package_id(&slugify("Hello World 2")).is_empty());
    }

    #[test]
    fn package_id_reports_uppercase_with_hint() {
        let issues = validate_package_id("MyPackage");
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("lowercase"));
    }

    #[test]
    fn reserved_windows_names_are_rejected() {
        let mut issues = Vec::new();
        validate_folder_name("con", &mut issues);
        validate_folder_name("Trailing.", &mut issues);
        assert_eq!(issues.len(), 2);
    }
}
