use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{CommandError, CommandResult};

pub const SERVER_NAME: &str = "nanosflow";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientKind {
    ClaudeCode,
    Cursor,
    VsCode,
    Codex,
    GeminiCli,
    Windsurf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallScope {
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SetupState {
    Configured,
    Outdated,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    label: String,
    language: &'static str,
    text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientSetup {
    kind: ClientKind,
    name: &'static str,
    detected: bool,
    state: SetupState,
    supports_project_scope: bool,
    one_click: &'static str,
    snippets: Vec<Snippet>,
}

pub fn display_name(client: &str) -> String {
    let lower = client.to_lowercase();
    let known = [
        ("claude-code", "Claude Code"),
        ("claude code", "Claude Code"),
        ("cursor", "Cursor"),
        ("visual studio code", "VS Code"),
        ("vscode", "VS Code"),
        ("codex", "Codex"),
        ("gemini", "Gemini CLI"),
        ("windsurf", "Windsurf"),
    ];
    known
        .iter()
        .find(|(needle, _)| lower.contains(needle))
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| client.to_owned())
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".codex"))
}

fn vscode_user_directory() -> PathBuf {
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData/Roaming")).join("Code/User")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Code/User")
    } else {
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("Code/User")
    }
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(target_os = "windows") {
        vec![format!("{name}.exe"), format!("{name}.cmd")]
    } else {
        vec![name.to_owned()]
    };
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH").map(|path| std::env::split_paths(&path).collect()).unwrap_or_default();
    let home = home();
    directories.extend([
        home.join(".local/bin"),
        home.join(".npm-global/bin"),
        home.join(".bun/bin"),
        home.join(".cargo/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
    ]);
    if let Some(app_data) = std::env::var_os("APPDATA") {
        directories.push(PathBuf::from(app_data).join("npm"));
    }
    directories
        .into_iter()
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|candidate| candidate.is_file())
}

fn read_json(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_json(path: &Path, value: &Value) -> CommandResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| file_error(parent, error))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|error| CommandError::new("Setup failed", error.to_string()))?;
    std::fs::write(path, format!("{text}\n")).map_err(|error| file_error(path, error))
}

fn file_error(path: &Path, error: std::io::Error) -> CommandError {
    CommandError::new("Setup failed", format!("{} could not be written: {error}", path.display()))
}

fn merge_json_server(path: &Path, section: &str, entry: Value) -> CommandResult<()> {
    let mut document = read_json(path);
    if !document.is_object() {
        return Err(CommandError::new(
            "Setup failed",
            format!("{} does not contain a JSON object. Fix the file or add the server by hand.", path.display()),
        ));
    }
    let servers = document
        .as_object_mut()
        .expect("checked above")
        .entry(section)
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        *servers = json!({});
    }
    servers[SERVER_NAME] = entry;
    write_json(path, &document)
}

fn json_state(path: &Path, section: &str, key: &str, url: &str) -> SetupState {
    let document = read_json(path);
    match document.get(section).and_then(|servers| servers.get(SERVER_NAME)) {
        Some(entry) if entry.get(key).and_then(Value::as_str) == Some(url) => SetupState::Configured,
        Some(_) => SetupState::Outdated,
        None => SetupState::Missing,
    }
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn shell_quote(text: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

fn snippet(label: impl Into<String>, language: &'static str, text: impl Into<String>) -> Snippet {
    Snippet {
        label: label.into(),
        language,
        text: text.into(),
    }
}

fn claude_project_file(project_root: &Path) -> PathBuf {
    project_root.join(".mcp.json")
}

fn codex_config() -> PathBuf {
    codex_home().join("config.toml")
}

fn codex_state(url: &str) -> SetupState {
    let Ok(text) = std::fs::read_to_string(codex_config()) else {
        return SetupState::Missing;
    };
    let Ok(document) = text.parse::<toml_edit::DocumentMut>() else {
        return SetupState::Missing;
    };
    match document.get("mcp_servers").and_then(|servers| servers.get(SERVER_NAME)) {
        Some(entry) if entry.get("url").and_then(|value| value.as_str()) == Some(url) => SetupState::Configured,
        Some(_) => SetupState::Outdated,
        None => SetupState::Missing,
    }
}

fn gemini_settings() -> PathBuf {
    home().join(".gemini/settings.json")
}

fn windsurf_config() -> PathBuf {
    home().join(".codeium/windsurf/mcp_config.json")
}

pub fn setups(url: &str, scope: InstallScope, project_root: Option<&Path>) -> Vec<ClientSetup> {
    let home = home();
    let claude_state = match (scope, project_root) {
        (InstallScope::Project, Some(root)) => json_state(&claude_project_file(root), "mcpServers", "url", url),
        _ => json_state(&home.join(".claude.json"), "mcpServers", "url", url),
    };
    let claude_command = match (scope, project_root) {
        (InstallScope::Project, Some(root)) => format!(
            "cd {} && claude mcp add --transport http --scope project {SERVER_NAME} {url}",
            shell_quote(&root.to_string_lossy())
        ),
        _ => format!("claude mcp add --transport http --scope user {SERVER_NAME} {url}"),
    };
    let vscode_entry = json!({"name": SERVER_NAME, "type": "http", "url": url});

    vec![
        ClientSetup {
            kind: ClientKind::ClaudeCode,
            name: "Claude Code",
            detected: find_executable("claude").is_some() || home.join(".claude").is_dir(),
            state: claude_state,
            supports_project_scope: true,
            one_click: if scope == InstallScope::Project { "Write .mcp.json" } else { "Add to Claude Code" },
            snippets: vec![
                snippet("Run in a terminal", "shell", claude_command),
                snippet(
                    if scope == InstallScope::Project { "Or put this into .mcp.json in the project folder" } else { "Project file .mcp.json" },
                    "json",
                    pretty(&json!({"mcpServers": {SERVER_NAME: {"type": "http", "url": url}}})),
                ),
            ],
        },
        ClientSetup {
            kind: ClientKind::Cursor,
            name: "Cursor",
            detected: home.join(".cursor").is_dir(),
            state: json_state(&home.join(".cursor/mcp.json"), "mcpServers", "url", url),
            supports_project_scope: false,
            one_click: "Open in Cursor",
            snippets: vec![snippet(
                "Add to ~/.cursor/mcp.json",
                "json",
                pretty(&json!({"mcpServers": {SERVER_NAME: {"url": url}}})),
            )],
        },
        ClientSetup {
            kind: ClientKind::VsCode,
            name: "VS Code",
            detected: find_executable("code").is_some() || vscode_user_directory().is_dir(),
            state: json_state(&vscode_user_directory().join("mcp.json"), "servers", "url", url),
            supports_project_scope: false,
            one_click: "Open in VS Code",
            snippets: vec![snippet(
                "Run in a terminal",
                "shell",
                format!("code --add-mcp {}", shell_quote(&vscode_entry.to_string())),
            )],
        },
        ClientSetup {
            kind: ClientKind::Codex,
            name: "Codex",
            detected: find_executable("codex").is_some() || codex_home().is_dir(),
            state: codex_state(url),
            supports_project_scope: false,
            one_click: "Add to Codex",
            snippets: vec![
                snippet("Run in a terminal", "shell", format!("codex mcp add {SERVER_NAME} --url {url}")),
                snippet("Or add to ~/.codex/config.toml", "toml", format!("[mcp_servers.{SERVER_NAME}]\nurl = \"{url}\"")),
            ],
        },
        ClientSetup {
            kind: ClientKind::GeminiCli,
            name: "Gemini CLI",
            detected: find_executable("gemini").is_some() || home.join(".gemini").is_dir(),
            state: json_state(&gemini_settings(), "mcpServers", "httpUrl", url),
            supports_project_scope: false,
            one_click: "Add to Gemini CLI",
            snippets: vec![snippet(
                "Run in a terminal",
                "shell",
                format!("gemini mcp add --scope user --transport http {SERVER_NAME} {url}"),
            )],
        },
        ClientSetup {
            kind: ClientKind::Windsurf,
            name: "Windsurf",
            detected: home.join(".codeium/windsurf").is_dir(),
            state: json_state(&windsurf_config(), "mcpServers", "serverUrl", url),
            supports_project_scope: false,
            one_click: "Add to Windsurf",
            snippets: vec![snippet(
                "Add to ~/.codeium/windsurf/mcp_config.json",
                "json",
                pretty(&json!({"mcpServers": {SERVER_NAME: {"serverUrl": url}}})),
            )],
        },
    ]
}

pub enum InstallAction {
    Done(String),
    OpenUrl(String, String),
}

fn run_claude(arguments: &[&str]) -> CommandResult<std::process::Output> {
    let executable = find_executable("claude").ok_or_else(|| {
        CommandError::new(
            "Claude Code not found",
            "The claude command is not installed or not on your PATH. Copy the command and run it in a terminal instead.",
        )
    })?;
    Command::new(executable)
        .args(arguments)
        .output()
        .map_err(|error| CommandError::new("Claude Code did not start", error.to_string()))
}

fn install_codex(url: &str) -> CommandResult<()> {
    let path = codex_config();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut document = text.parse::<toml_edit::DocumentMut>().map_err(|error| {
        CommandError::new("Setup failed", format!("{} could not be read: {error}. Add the server by hand.", path.display()))
    })?;
    if !document.contains_key("mcp_servers") {
        let mut servers = toml_edit::Table::new();
        servers.set_implicit(true);
        document.insert("mcp_servers", toml_edit::Item::Table(servers));
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| CommandError::new("Setup failed", "mcp_servers in the Codex config is not a table. Add the server by hand."))?;
    let mut entry = toml_edit::Table::new();
    entry.insert("url", toml_edit::value(url));
    servers.insert(SERVER_NAME, toml_edit::Item::Table(entry));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| file_error(parent, error))?;
    }
    std::fs::write(&path, document.to_string()).map_err(|error| file_error(&path, error))
}

pub fn install(kind: ClientKind, scope: InstallScope, url: &str, project_root: Option<&Path>) -> CommandResult<InstallAction> {
    match kind {
        ClientKind::ClaudeCode => match (scope, project_root) {
            (InstallScope::Project, Some(root)) => {
                let path = claude_project_file(root);
                merge_json_server(&path, "mcpServers", json!({"type": "http", "url": url}))?;
                Ok(InstallAction::Done(format!(
                    "Wrote {}. Claude Code asks once to approve the server when you start it in this folder.",
                    path.display()
                )))
            }
            (InstallScope::Project, None) => Err(CommandError::new("No project open", "Open a project to add NanosFlow to its .mcp.json.")),
            (InstallScope::User, _) => {
                let _ = run_claude(&["mcp", "remove", SERVER_NAME, "--scope", "user"]);
                let output = run_claude(&["mcp", "add", "--transport", "http", "--scope", "user", SERVER_NAME, url])?;
                if output.status.success() {
                    Ok(InstallAction::Done("Claude Code can now use NanosFlow in every folder. Restart running Claude Code sessions.".to_owned()))
                } else {
                    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                    Err(CommandError::new("Claude Code refused the server", if message.is_empty() { "claude mcp add failed.".to_owned() } else { message }))
                }
            }
        },
        ClientKind::Cursor => {
            let config = base64::engine::general_purpose::STANDARD.encode(json!({"url": url}).to_string());
            Ok(InstallAction::OpenUrl(
                format!("cursor://anysphere.cursor-deeplink/mcp/install?name={SERVER_NAME}&config={}", percent_encode(&config)),
                "Cursor opens and asks you to confirm the server.".to_owned(),
            ))
        }
        ClientKind::VsCode => Ok(InstallAction::OpenUrl(
            format!("vscode:mcp/install?{}", percent_encode(&json!({"name": SERVER_NAME, "type": "http", "url": url}).to_string())),
            "VS Code opens and shows the server for you to install.".to_owned(),
        )),
        ClientKind::Codex => {
            install_codex(url)?;
            Ok(InstallAction::Done(format!("Added NanosFlow to {}. Restart Codex to use it.", codex_config().display())))
        }
        ClientKind::GeminiCli => {
            merge_json_server(&gemini_settings(), "mcpServers", json!({"httpUrl": url}))?;
            Ok(InstallAction::Done(format!("Added NanosFlow to {}.", gemini_settings().display())))
        }
        ClientKind::Windsurf => {
            merge_json_server(&windsurf_config(), "mcpServers", json!({"serverUrl": url}))?;
            Ok(InstallAction::Done(format!("Added NanosFlow to {}. Refresh the MCP servers in Windsurf.", windsurf_config().display())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_servers_without_touching_other_entries() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("mcp.json");
        std::fs::write(&path, r#"{"mcpServers":{"other":{"url":"x"}},"theme":"dark"}"#).unwrap();
        merge_json_server(&path, "mcpServers", json!({"url": "http://127.0.0.1:7920/mcp"})).unwrap();
        let document = read_json(&path);
        assert_eq!(document["mcpServers"]["other"]["url"], "x");
        assert_eq!(document["theme"], "dark");
        assert_eq!(json_state(&path, "mcpServers", "url", "http://127.0.0.1:7920/mcp"), SetupState::Configured);
        assert_eq!(json_state(&path, "mcpServers", "url", "http://127.0.0.1:7921/mcp"), SetupState::Outdated);
    }

    #[test]
    fn names_known_clients() {
        assert_eq!(display_name("claude-code"), "Claude Code");
        assert_eq!(display_name("Visual Studio Code"), "VS Code");
        assert_eq!(display_name("codex-mcp-client"), "Codex");
        assert_eq!(display_name("my-agent"), "my-agent");
        assert_eq!(percent_encode("a b/+"), "a%20b%2F%2B");
    }
}
