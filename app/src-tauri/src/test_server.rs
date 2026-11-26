use std::io::{BufRead, BufReader, Read, Write};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use nf_api::ApiChannel;
use nf_compiler::BuildReport;
use nf_project::{Project, ServerBranch, SteamApp, TestServerSettings};
use nf_server::{InstallProgress, LaunchPlan};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{CommandError, CommandResult};
use crate::graph_commands::CatalogCache;

pub const TEST_SERVER_EVENT: &str = "test-server";
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const LOCAL_ADDRESS: &str = "127.0.0.1";

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
enum ServerMessage {
    Output { line: String },
    State { running: bool, exit_code: Option<i32> },
    Install { line: String, progress: Option<InstallProgress> },
    Installed { success: bool, message: String },
}

struct Running {
    id: u64,
    child: Child,
    stdin: ChildStdin,
    directory: PathBuf,
    arguments: Vec<String>,
}

#[derive(Default)]
struct Inner {
    running: Option<Running>,
    installing: bool,
    next_id: u64,
}

#[derive(Default)]
pub struct TestServerState {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    directory: Option<PathBuf>,
    steam_app: SteamApp,
    branch: ServerBranch,
    port: u16,
    map: Option<String>,
    game_mode: Option<String>,
    download_missing: bool,
}

impl From<TestServerSettings> for SettingsView {
    fn from(settings: TestServerSettings) -> SettingsView {
        SettingsView {
            directory: settings.directory,
            steam_app: settings.steam_app,
            branch: settings.branch,
            port: settings.port,
            map: settings.map,
            game_mode: settings.game_mode,
            download_missing: settings.download_missing,
        }
    }
}

impl From<SettingsView> for TestServerSettings {
    fn from(view: SettingsView) -> TestServerSettings {
        TestServerSettings {
            directory: view.directory,
            steam_app: view.steam_app,
            branch: view.branch,
            port: view.port,
            map: view.map.filter(|map| !map.trim().is_empty()).map(|map| map.trim().to_owned()),
            game_mode: view.game_mode.filter(|mode| !mode.is_empty()),
            download_missing: view.download_missing,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestServerStatus {
    directory: PathBuf,
    global_directory: PathBuf,
    uses_global: bool,
    installed: bool,
    installing: bool,
    running: bool,
    running_here: bool,
    settings: SettingsView,
    plan: LaunchPlan,
    game_modes: Vec<String>,
    address: String,
    log_file: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlayAction {
    BuildFailed,
    Started,
    Reloaded,
    Restarted,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayResult {
    action: PlayAction,
    report: BuildReport,
    address: String,
}

impl TestServerState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        lock(&self.inner)
    }

    pub fn shutdown(&self) {
        let running = self.lock().running.take();
        if let Some(running) = running {
            stop_process(running);
        }
    }
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn emit(app: &AppHandle, message: ServerMessage) {
    let _ = app.emit(TEST_SERVER_EVENT, message);
}

fn app_data(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|error| CommandError::new("No data folder", format!("NanosFlow could not find its data folder: {error}")))
}

fn global_directory(app: &AppHandle) -> CommandResult<PathBuf> {
    Ok(app_data(app)?.join("server"))
}

fn server_directory(app: &AppHandle, settings: &TestServerSettings) -> CommandResult<PathBuf> {
    match &settings.directory {
        Some(directory) => Ok(directory.clone()),
        None => global_directory(app),
    }
}

fn address(settings: &TestServerSettings) -> String {
    format!("{LOCAL_ADDRESS}:{}", settings.port)
}

fn status(app: &AppHandle, state: &TestServerState, project: &Project) -> CommandResult<TestServerStatus> {
    let settings = project.local_settings().test_server.clone();
    let directory = server_directory(app, &settings)?;
    let inner = state.lock();
    let running_here = inner.running.as_ref().is_some_and(|running| running.directory == directory);
    Ok(TestServerStatus {
        global_directory: global_directory(app)?,
        uses_global: settings.directory.is_none(),
        installed: nf_server::is_installed(&directory),
        installing: inner.installing,
        running: inner.running.is_some(),
        running_here,
        plan: nf_server::launch_plan(project.packages(), &settings),
        game_modes: project
            .packages()
            .iter()
            .filter(|package| package.package_type == nf_project::PackageType::GameMode)
            .map(|package| package.folder.clone())
            .collect(),
        address: address(&settings),
        log_file: nf_server::log_file(&directory),
        directory,
        settings: settings.into(),
    })
}

#[tauri::command]
pub fn test_server_status(app: AppHandle, state: State<'_, TestServerState>, project_file: PathBuf) -> CommandResult<TestServerStatus> {
    status(&app, &state, &Project::open(&project_file)?)
}

#[tauri::command]
pub fn update_test_server_settings(
    app: AppHandle,
    state: State<'_, TestServerState>,
    project_file: PathBuf,
    settings: SettingsView,
) -> CommandResult<TestServerStatus> {
    let project = Project::open(&project_file)?;
    let settings = TestServerSettings::from(settings);
    if !(1024..u16::MAX).contains(&settings.port) {
        return Err(CommandError::new("Port not allowed", "Pick a port between 1024 and 65534. The query port uses the next one."));
    }
    if let Some(directory) = &settings.directory {
        if !directory.is_absolute() {
            return Err(CommandError::new("Folder not allowed", "Choose the server folder with the folder picker."));
        }
        if directory.starts_with(project.root()) {
            return Err(CommandError::new(
                "Folder not allowed",
                "The test server cannot live inside the project, because its packages and logs would end up in version control.",
            ));
        }
    }
    let mut local = project.local_settings().clone();
    local.test_server = settings;
    local.write(project.root())?;
    status(&app, &state, &Project::open(&project_file)?)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let candidates: Vec<String> = if cfg!(windows) { vec![format!("{name}.exe")] } else { vec![name.to_owned()] };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|directory| candidates.iter().map(move |candidate| directory.join(candidate)))
        .find(|path| path.is_file())
}

fn run_logged(app: &AppHandle, mut command: Command) -> Result<bool, String> {
    hide_console(&mut command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start {:?}: {error}", command.get_program()))?;
    let mut succeeded = false;
    if let Some(stderr) = child.stderr.take() {
        let app = app.clone();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                emit(&app, ServerMessage::Install { line: nf_server::strip_ansi(&line), progress: None });
            }
        });
    }
    if let Some(mut stdout) = child.stdout.take() {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(read) = stdout.read(&mut chunk) {
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
            while let Some(end) = buffer.iter().position(|byte| *byte == b'\n' || *byte == b'\r') {
                let line = nf_server::strip_ansi(String::from_utf8_lossy(&buffer[..end]).trim());
                buffer.drain(..=end);
                if line.is_empty() {
                    continue;
                }
                succeeded |= nf_server::install_succeeded(&line);
                let progress = nf_server::install_progress(&line);
                emit(app, ServerMessage::Install { line, progress });
            }
        }
    }
    let exit = child.wait().map_err(|error| error.to_string())?;
    Ok(succeeded || exit.success())
}

fn steamcmd(app: &AppHandle) -> Result<PathBuf, String> {
    let download = nf_server::steamcmd_download();
    let tools = app_data(app).map_err(|error| error.message)?.join("steamcmd");
    let bundled = tools.join(download.executable);
    if bundled.is_file() {
        return Ok(bundled);
    }
    if let Some(system) = find_on_path("steamcmd") {
        return Ok(system);
    }
    std::fs::create_dir_all(&tools).map_err(|error| format!("Could not create {}: {error}", tools.display()))?;
    let archive = tools.join(download.archive);
    emit(app, ServerMessage::Install { line: format!("Downloading SteamCMD from {}", download.url), progress: None });
    let mut curl = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    curl.arg("-fsSL").arg("-o").arg(&archive).arg(download.url);
    if !run_logged(app, curl)? {
        return Err("SteamCMD could not be downloaded. Check your internet connection, or install SteamCMD yourself.".to_owned());
    }
    let mut tar = Command::new("tar");
    tar.arg("-xf").arg(&archive).arg("-C").arg(&tools);
    if !run_logged(app, tar)? || !bundled.is_file() {
        return Err("SteamCMD was downloaded but could not be unpacked.".to_owned());
    }
    let _ = std::fs::remove_file(&archive);
    make_executable(&bundled);
    Ok(bundled)
}

fn install(app: &AppHandle, directory: &Path, branch: ServerBranch) -> Result<(), String> {
    let steamcmd = steamcmd(app)?;
    std::fs::create_dir_all(directory).map_err(|error| format!("Could not create {}: {error}", directory.display()))?;
    emit(app, ServerMessage::Install { line: format!("Installing nanos world server into {}", directory.display()), progress: None });
    let mut command = Command::new(&steamcmd);
    command.args(nf_server::install_arguments(directory, branch));
    let finished = run_logged(app, command)?;
    if finished && nf_server::is_installed(directory) {
        Ok(())
    } else {
        Err("SteamCMD finished, but the server is not there. The log above shows what went wrong.".to_owned())
    }
}

#[tauri::command]
pub fn install_test_server(app: AppHandle, state: State<'_, TestServerState>, project_file: PathBuf) -> CommandResult<()> {
    let project = Project::open(&project_file)?;
    let settings = project.local_settings().test_server.clone();
    let directory = server_directory(&app, &settings)?;
    {
        let mut inner = state.lock();
        if inner.installing {
            return Err(CommandError::new("Already installing", "The test server is already being installed."));
        }
        if inner.running.as_ref().is_some_and(|running| running.directory == directory) {
            return Err(CommandError::new("Server is running", "Stop the test server before updating it."));
        }
        inner.installing = true;
    }
    let inner = state.inner.clone();
    thread::spawn(move || {
        let result = install(&app, &directory, settings.branch);
        lock(&inner).installing = false;
        let message = match result {
            Ok(()) => ServerMessage::Installed { success: true, message: "The test server is installed.".to_owned() },
            Err(message) => ServerMessage::Installed { success: false, message },
        };
        emit(&app, message);
    });
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = std::fs::metadata(path) {
        let mut permissions = metadata.permissions();
        if permissions.mode() & 0o111 == 0 {
            permissions.set_mode(permissions.mode() | 0o755);
            let _ = std::fs::set_permissions(path, permissions);
        }
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

fn forward_output(app: AppHandle, inner: Arc<Mutex<Inner>>, id: u64, output: impl Read + Send + 'static, owns_exit: bool) {
    thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            let line = nf_server::strip_ansi(&line);
            if !line.trim().is_empty() {
                emit(&app, ServerMessage::Output { line });
            }
        }
        if !owns_exit {
            return;
        }
        let finished = {
            let mut inner = lock(&inner);
            if inner.running.as_ref().is_some_and(|running| running.id == id) { inner.running.take() } else { None }
        };
        if let Some(mut running) = finished {
            let exit_code = running.child.wait().ok().and_then(|status| status.code());
            emit(&app, ServerMessage::State { running: false, exit_code });
            app.state::<crate::session::SessionState>().server_changed(&app, None);
        }
    });
}

fn start(app: &AppHandle, state: &TestServerState, directory: &Path, arguments: Vec<String>, port: u16, steam_app: SteamApp) -> CommandResult<()> {
    if UdpSocket::bind(("0.0.0.0", port)).is_err() {
        return Err(CommandError::new(
            "Port in use",
            format!("Port {port} is already used, probably by another nanos world server. Stop it, or pick another port in the test server settings."),
        ));
    }
    let executable = nf_server::executable(directory);
    make_executable(&executable);
    let mut command = Command::new(&executable);
    command
        .args(&arguments)
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| CommandError::new("Server did not start", format!("{} could not be started: {error}", executable.display())))?;
    let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Err(CommandError::new("Server did not start", "The server console could not be connected."));
    };
    let id = {
        let mut inner = state.lock();
        inner.next_id += 1;
        let id = inner.next_id;
        inner.running = Some(Running {
            id,
            child,
            stdin,
            directory: directory.to_path_buf(),
            arguments,
        });
        id
    };
    forward_output(app.clone(), state.inner.clone(), id, stderr, false);
    forward_output(app.clone(), state.inner.clone(), id, stdout, true);
    emit(app, ServerMessage::State { running: true, exit_code: None });
    app.state::<crate::session::SessionState>().server_changed(app, Some(steam_app));
    Ok(())
}

fn stop_process(mut running: Running) -> Option<i32> {
    let _ = writeln!(running.stdin, "stop");
    let _ = running.stdin.flush();
    let started = Instant::now();
    while started.elapsed() < STOP_TIMEOUT {
        if let Ok(Some(status)) = running.child.try_wait() {
            return status.code();
        }
        thread::sleep(Duration::from_millis(100));
    }
    let _ = running.child.kill();
    running.child.wait().ok().and_then(|status| status.code())
}

fn stop(app: &AppHandle, state: &TestServerState) -> bool {
    let running = state.lock().running.take();
    match running {
        Some(running) => {
            let exit_code = stop_process(running);
            emit(app, ServerMessage::State { running: false, exit_code });
            app.state::<crate::session::SessionState>().server_changed(app, None);
            true
        }
        None => false,
    }
}

#[tauri::command]
pub async fn stop_test_server(app: AppHandle, state: State<'_, TestServerState>) -> CommandResult<bool> {
    Ok(stop(&app, &state))
}

#[tauri::command]
pub fn send_test_server_command(app: AppHandle, state: State<'_, TestServerState>, line: String) -> CommandResult<()> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }
    let mut inner = state.lock();
    let Some(running) = inner.running.as_mut() else {
        return Err(CommandError::new("Server is not running", "Start the test server with Play first."));
    };
    writeln!(running.stdin, "{line}")
        .and_then(|()| running.stdin.flush())
        .map_err(|error| CommandError::new("Command not sent", format!("The server console did not accept the command: {error}")))?;
    drop(inner);
    emit(&app, ServerMessage::Output { line: format!("> {line}") });
    Ok(())
}

#[tauri::command]
pub async fn play_test_server(
    app: AppHandle,
    state: State<'_, TestServerState>,
    cache: State<'_, CatalogCache>,
    channel: ApiChannel,
    project_file: PathBuf,
    restart: bool,
) -> CommandResult<PlayResult> {
    let project = Project::open(&project_file)?;
    let settings = project.local_settings().test_server.clone();
    let directory = server_directory(&app, &settings)?;
    if state.lock().installing {
        return Err(CommandError::new("Still installing", "Wait until the test server is installed, then press Play again."));
    }
    if !nf_server::is_installed(&directory) {
        return Err(CommandError::new(
            "No test server yet",
            format!("There is no nanos world server in {}. Install it in the test server panel first.", directory.display()),
        ));
    }
    let cached = cache.get(channel)?;
    let report = nf_compiler::build_project_into(&project, &cached.catalog, nf_server::packages_directory(&directory))?;
    let address = address(&settings);
    if report.error_count > 0 {
        return Ok(PlayResult { action: PlayAction::BuildFailed, report, address });
    }
    let plan = nf_server::launch_plan(project.packages(), &settings);
    let built: Vec<String> = report.packages.iter().filter(|package| package.written).map(|package| package.folder.clone()).collect();

    let reload = {
        let mut inner = state.lock();
        match inner.running.as_mut() {
            Some(running) if !restart && running.directory == directory && running.arguments == plan.arguments => {
                if let Some(command) = plan.reload_command(&built) {
                    writeln!(running.stdin, "{command}").and_then(|()| running.stdin.flush()).map_err(|error| {
                        CommandError::new("Reload failed", format!("The server console did not accept the reload: {error}"))
                    })?;
                    emit(&app, ServerMessage::Output { line: format!("> {command}") });
                }
                true
            }
            _ => false,
        }
    };
    if reload {
        return Ok(PlayResult { action: PlayAction::Reloaded, report, address });
    }
    let was_running = stop(&app, &state);
    start(&app, &state, &directory, plan.arguments, settings.port, settings.steam_app)?;
    Ok(PlayResult {
        action: if was_running { PlayAction::Restarted } else { PlayAction::Started },
        report,
        address,
    })
}
