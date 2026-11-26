use std::path::{Path, PathBuf};

use nf_project::SteamApp;

const LIBRARY_FILE: [&str; 2] = ["steamapps", "libraryfolders.vdf"];
const APPS_DIRECTORY: &str = "steamapps";
const GAME_NAME: &str = "nanos world";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInstall {
    pub app_id: u32,
    pub name: String,
    pub playtest: bool,
}

pub fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(directory) = std::env::var_os(variable) {
                roots.push(PathBuf::from(directory).join("Steam"));
            }
        }
    } else if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        roots.push(home.join("Library/Application Support/Steam"));
    }
    roots
}

fn quoted_pair(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.trim().split('"').filter(|part| !part.trim().is_empty());
    Some((parts.next()?, parts.next()?))
}

fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![root.to_path_buf()];
    let file = LIBRARY_FILE.iter().fold(root.to_path_buf(), |path, part| path.join(part));
    if let Ok(content) = std::fs::read_to_string(file) {
        for (key, value) in content.lines().filter_map(quoted_pair) {
            if key == "path" {
                folders.push(PathBuf::from(value.replace("\\\\", "\\")));
            }
        }
    }
    folders
}

fn read_manifest(path: &Path) -> Option<ClientInstall> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut app_id = None;
    let mut name = None;
    for (key, value) in content.lines().filter_map(quoted_pair) {
        match key {
            "appid" if app_id.is_none() => app_id = value.parse().ok(),
            "name" if name.is_none() => name = Some(value.to_owned()),
            _ => {}
        }
    }
    let name = name?;
    let lower = name.to_lowercase();
    if !lower.contains(GAME_NAME) || lower.contains("server") {
        return None;
    }
    Some(ClientInstall {
        app_id: app_id?,
        playtest: lower.contains("playtest"),
        name,
    })
}

pub fn installed_clients(roots: &[PathBuf]) -> Vec<ClientInstall> {
    let mut clients: Vec<ClientInstall> = Vec::new();
    for root in roots {
        for folder in library_folders(root) {
            let Ok(entries) = std::fs::read_dir(folder.join(APPS_DIRECTORY)) else {
                continue;
            };
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name = file_name.to_string_lossy();
                if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
                    continue;
                }
                if let Some(client) = read_manifest(&entry.path())
                    && !clients.iter().any(|known| known.app_id == client.app_id)
                {
                    clients.push(client);
                }
            }
        }
    }
    clients.sort_by_key(|client| client.app_id);
    clients
}

pub fn choose_client(clients: &[ClientInstall], steam_app: SteamApp) -> Option<&ClientInstall> {
    let wants_playtest = steam_app == SteamApp::Playtest;
    clients
        .iter()
        .find(|client| client.playtest == wants_playtest)
        .or_else(|| if clients.len() == 1 { clients.first() } else { None })
}

pub fn connect_url(app_id: u32, address: &str) -> String {
    format!("steam://run/{app_id}//+connect%20{address}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(directory: &Path, app_id: u32, name: &str) {
        std::fs::create_dir_all(directory.join(APPS_DIRECTORY)).unwrap();
        std::fs::write(
            directory.join(APPS_DIRECTORY).join(format!("appmanifest_{app_id}.acf")),
            format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{app_id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"installdir\"\t\t\"x\"\n}}\n"),
        )
        .unwrap();
    }

    #[test]
    fn finds_clients_in_every_library_and_picks_the_right_app() {
        let steam = tempfile::tempdir().unwrap();
        let library = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(steam.path().join(APPS_DIRECTORY)).unwrap();
        std::fs::write(
            steam.path().join("steamapps/libraryfolders.vdf"),
            format!("\"libraryfolders\"\n{{\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n", library.path().display()),
        )
        .unwrap();
        manifest(steam.path(), 1841660, "nanos world™");
        manifest(library.path(), 2000000, "nanos world™ Playtest");
        manifest(library.path(), 1936830, "nanos world Dedicated Server");
        manifest(library.path(), 440, "Team Fortress 2");

        let clients = installed_clients(&[steam.path().to_path_buf()]);
        assert_eq!(clients.iter().map(|client| (client.app_id, client.playtest)).collect::<Vec<_>>(), [(1841660, false), (2000000, true)]);
        assert_eq!(choose_client(&clients, SteamApp::Playtest).map(|client| client.app_id), Some(2000000));
        assert_eq!(choose_client(&clients, SteamApp::Game).map(|client| client.app_id), Some(1841660));
        assert_eq!(choose_client(&clients[..1], SteamApp::Playtest).map(|client| client.app_id), Some(1841660));
        assert_eq!(choose_client(&[], SteamApp::Game), None);
        assert_eq!(connect_url(1841660, "127.0.0.1:7787"), "steam://run/1841660//+connect%20127.0.0.1:7787/");
    }
}
