mod client;

use std::path::{Path, PathBuf};

use nf_project::{PackageSummary, PackageType, ServerBranch, TestServerSettings};
use serde::Serialize;

pub use client::{ClientInstall, choose_client, connect_url, installed_clients, steam_roots};

pub const SERVER_APP_ID: u32 = 1936830;
pub const PACKAGES_DIRECTORY: &str = "Packages";
pub const DEFAULT_MAP: &str = "default-blank-map";
pub const SERVER_NAME: &str = "NanosFlow Test Server";
const LOG_FILE: [&str; 2] = [".logs", "NanosWorldCore.log"];

pub fn executable_name() -> &'static str {
    if cfg!(windows) { "NanosWorldServer.exe" } else { "NanosWorldServer.sh" }
}

pub fn executable(directory: &Path) -> PathBuf {
    directory.join(executable_name())
}

pub fn is_installed(directory: &Path) -> bool {
    executable(directory).is_file()
}

pub fn packages_directory(directory: &Path) -> PathBuf {
    directory.join(PACKAGES_DIRECTORY)
}

pub fn log_file(directory: &Path) -> PathBuf {
    LOG_FILE.iter().fold(directory.to_path_buf(), |path, part| path.join(part))
}

pub struct SteamCmdDownload {
    pub url: &'static str,
    pub archive: &'static str,
    pub executable: &'static str,
}

pub fn steamcmd_download() -> SteamCmdDownload {
    if cfg!(windows) {
        SteamCmdDownload {
            url: "https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip",
            archive: "steamcmd.zip",
            executable: "steamcmd.exe",
        }
    } else {
        SteamCmdDownload {
            url: "https://steamcdn-a.akamaihd.net/client/installer/steamcmd_linux.tar.gz",
            archive: "steamcmd_linux.tar.gz",
            executable: "steamcmd.sh",
        }
    }
}

pub fn install_arguments(directory: &Path, branch: ServerBranch) -> Vec<String> {
    vec![
        "+force_install_dir".to_owned(),
        directory.display().to_string(),
        "+login".to_owned(),
        "anonymous".to_owned(),
        format!("+app_update {SERVER_APP_ID} -beta {}", branch.steam_branch()),
        "validate".to_owned(),
        "+quit".to_owned(),
    ]
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub phase: String,
    pub percent: f32,
}

pub fn install_progress(line: &str) -> Option<InstallProgress> {
    let (before, after) = line.split_once("progress:")?;
    let percent = after.split_whitespace().next()?.parse::<f32>().ok()?;
    let phase = before
        .rsplit_once(')')
        .map_or(before, |(_, phase)| phase)
        .trim()
        .trim_end_matches(',')
        .trim();
    Some(InstallProgress {
        phase: phase.to_owned(),
        percent: percent.clamp(0.0, 100.0),
    })
}

pub fn install_succeeded(line: &str) -> bool {
    line.contains("Success!") && line.contains(&SERVER_APP_ID.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub map: String,
    pub project_map: Option<String>,
    pub game_mode: Option<String>,
    pub packages: Vec<String>,
    pub ignored_game_modes: Vec<String>,
    pub load_order: Vec<String>,
    pub arguments: Vec<String>,
}

impl LaunchPlan {
    pub fn reload_command(&self, built: &[String]) -> Option<String> {
        let folders: Vec<&String> = self.load_order.iter().filter(|folder| built.contains(folder)).collect();
        let folders: Vec<&str> = folders.into_iter().map(String::as_str).collect();
        (!folders.is_empty()).then(|| format!("package reload {}", folders.join(" ")))
    }
}

fn packages_by_folder(packages: &[PackageSummary]) -> Vec<(String, Vec<String>)> {
    packages.iter().map(|package| (package.folder.clone(), package.requirements.clone())).collect()
}

fn load_order(requirements: Vec<(String, Vec<String>)>, loaded: &[String]) -> Vec<String> {
    fn visit(folder: &str, requirements: &[(String, Vec<String>)], visiting: &mut Vec<String>, order: &mut Vec<String>) {
        if order.iter().any(|done| done == folder) || visiting.iter().any(|open| open == folder) {
            return;
        }
        let Some((_, required)) = requirements.iter().find(|(candidate, _)| candidate == folder) else {
            return;
        };
        visiting.push(folder.to_owned());
        for requirement in required {
            visit(requirement, requirements, visiting, order);
        }
        visiting.pop();
        order.push(folder.to_owned());
    }
    let mut order = Vec::new();
    for folder in loaded {
        visit(folder, &requirements, &mut Vec::new(), &mut order);
    }
    order
}

pub fn launch_plan(packages: &[PackageSummary], settings: &TestServerSettings) -> LaunchPlan {
    let requirements = packages_by_folder(packages);
    let of_type = |package_type: PackageType| -> Vec<String> {
        packages
            .iter()
            .filter(|package| package.package_type == package_type)
            .map(|package| package.folder.clone())
            .collect()
    };
    let game_modes = of_type(PackageType::GameMode);
    let game_mode = settings
        .game_mode
        .as_ref()
        .filter(|chosen| game_modes.contains(chosen))
        .or(game_modes.first())
        .cloned();
    let ignored_game_modes = game_modes.iter().filter(|folder| Some(*folder) != game_mode.as_ref()).cloned().collect();
    let packages = of_type(PackageType::Script);
    let project_map = of_type(PackageType::Map).into_iter().next();
    let map = project_map
        .clone()
        .or_else(|| settings.map.clone().filter(|map| !map.trim().is_empty()))
        .unwrap_or_else(|| DEFAULT_MAP.to_owned());

    let mut arguments = vec![
        "--name".to_owned(),
        SERVER_NAME.to_owned(),
        "--announce".to_owned(),
        "false".to_owned(),
        "--dedicated_server".to_owned(),
        "true".to_owned(),
        "--port".to_owned(),
        settings.port.to_string(),
        "--query_port".to_owned(),
        query_port(settings.port).to_string(),
        "--steam_app".to_owned(),
        settings.steam_app.argument().to_owned(),
        "--map".to_owned(),
        map.clone(),
    ];
    if let Some(game_mode) = &game_mode {
        arguments.extend(["--game_mode".to_owned(), game_mode.clone()]);
    }
    if !packages.is_empty() {
        arguments.extend(["--packages".to_owned(), packages.join(",")]);
    }
    if settings.download_missing {
        arguments.extend(["--vault_download".to_owned(), "missing".to_owned()]);
    }
    let loaded: Vec<String> = game_mode.iter().chain(&packages).chain(&project_map).cloned().collect();
    LaunchPlan {
        load_order: load_order(requirements, &loaded),
        map,
        project_map,
        game_mode,
        packages,
        ignored_game_modes,
        arguments,
    }
}

pub fn query_port(port: u16) -> u16 {
    if port == u16::MAX { port - 1 } else { port + 1 }
}

pub fn strip_ansi(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            output.push(character);
            continue;
        }
        if characters.peek() == Some(&'[') {
            characters.next();
            for code in characters.by_ref() {
                if code.is_ascii_alphabetic() || code == '~' {
                    break;
                }
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use nf_project::{Id, SteamApp};

    fn package(folder: &str, package_type: PackageType) -> PackageSummary {
        PackageSummary {
            id: Id::generate(),
            folder: folder.to_owned(),
            path: PathBuf::from(folder),
            title: folder.to_owned(),
            package_type,
            version: "1.0.0".to_owned(),
            requirements: Vec::new(),
            graphs: Vec::new(),
            folders: Vec::new(),
        }
    }

    #[test]
    fn plans_the_server_start_from_the_package_types() {
        let mut arena = package("arena", PackageType::GameMode);
        arena.requirements = vec!["hud".to_owned(), "default-vehicles".to_owned()];
        let packages = [
            arena,
            package("core", PackageType::Script),
            package("hud", PackageType::Script),
            package("other-mode", PackageType::GameMode),
        ];
        let settings = TestServerSettings {
            steam_app: SteamApp::Game,
            port: 7790,
            ..TestServerSettings::default()
        };
        let plan = launch_plan(&packages, &settings);
        assert_eq!(plan.game_mode.as_deref(), Some("arena"));
        assert_eq!(plan.ignored_game_modes, ["other-mode"]);
        assert_eq!(plan.map, DEFAULT_MAP);
        let joined = plan.arguments.join(" ");
        assert!(joined.contains("--steam_app game"), "{joined}");
        assert!(joined.contains("--port 7790 --query_port 7791"), "{joined}");
        assert!(joined.contains("--game_mode arena --packages core,hud --vault_download missing"), "{joined}");
        assert_eq!(plan.reload_command(&["arena".to_owned(), "hud".to_owned()]).as_deref(), Some("package reload hud arena"));
        assert_eq!(plan.reload_command(&[]), None);

        let chosen = launch_plan(
            &[packages[0].clone(), packages[3].clone(), package("island", PackageType::Map)],
            &TestServerSettings {
                game_mode: Some("other-mode".to_owned()),
                download_missing: false,
                ..TestServerSettings::default()
            },
        );
        assert_eq!(chosen.game_mode.as_deref(), Some("other-mode"));
        assert_eq!(chosen.map, "island");
        assert_eq!(chosen.load_order, ["other-mode", "island"]);
        assert!(!chosen.arguments.contains(&"--vault_download".to_owned()));
        assert!(chosen.arguments.join(" ").contains("--steam_app playtest"));
    }

    #[test]
    fn reads_steamcmd_progress_and_cleans_console_output() {
        let progress = install_progress(" Update state (0x61) downloading, progress: 37.53 (12345 / 67890)").unwrap();
        assert_eq!(progress.phase, "downloading");
        assert!((progress.percent - 37.53).abs() < 0.001);
        assert_eq!(install_progress("Loading Steam API...OK"), None);
        assert!(install_succeeded("Success! App '1936830' fully installed."));
        assert_eq!(strip_ansi("\u{1b}[1;31mError\u{1b}[0m in Main.lua:3"), "Error in Main.lua:3");
        let arguments = install_arguments(Path::new("/srv/nanos"), ServerBranch::BleedingEdge);
        assert_eq!(arguments[4], "+app_update 1936830 -beta bleeding-edge");
    }
}
