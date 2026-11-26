mod commands;
mod debugger;
#[cfg(target_os = "linux")]
mod desktop_integration;
mod error;
mod graph_commands;
mod launch;
mod mcp;
mod recent;
mod session;
mod test_server;

use std::sync::Mutex;

use tauri::{Emitter, Manager};

use crate::commands::LaunchState;
use crate::graph_commands::CatalogCache;
use crate::launch::find_project_argument;

pub const OPEN_PROJECT_EVENT: &str = "open-project-requested";

pub fn run() {
    #[cfg(target_os = "linux")]
    {
        desktop_integration::apply_application_id();
        desktop_integration::register();
    }

    let working_directory = std::env::current_dir().unwrap_or_default();
    let launch_project = find_project_argument(std::env::args(), &working_directory);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, arguments, working_directory| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let working_directory = std::path::PathBuf::from(working_directory);
            if let Some(path) = find_project_argument(arguments, &working_directory) {
                let _ = app.emit(OPEN_PROJECT_EVENT, path);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            app.manage(debugger::DebuggerState::start(app.handle()));
            mcp::start(app.handle());
            Ok(())
        })
        .manage(CatalogCache::default())
        .manage(test_server::TestServerState::default())
        .manage(session::SessionState::default())
        .manage(mcp::McpState::default())
        .manage(LaunchState {
            pending_project: Mutex::new(launch_project),
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::default_project_options,
            commands::slugify,
            commands::validate_new_project,
            commands::create_project,
            commands::open_project,
            commands::list_recent_projects,
            commands::remove_recent_project,
            commands::take_launch_project,
            commands::path_exists,
            commands::update_output_directory,
            graph_commands::node_catalog,
            graph_commands::load_graph,
            graph_commands::save_graph_changes,
            graph_commands::analyze_graph,
            graph_commands::graph_scope,
            graph_commands::update_function_signature,
            graph_commands::update_event,
            graph_commands::save_variables,
            graph_commands::create_method,
            graph_commands::create_dispatcher,
            graph_commands::update_interface,
            graph_commands::graph_conflicts,
            graph_commands::resolve_conflict,
            graph_commands::committed_graph,
            graph_commands::update_class,
            graph_commands::validate_entry_name,
            graph_commands::create_graph,
            graph_commands::update_package_requirements,
            graph_commands::create_folder,
            graph_commands::rename_entry,
            graph_commands::move_entry,
            graph_commands::delete_entry,
            graph_commands::build_project,
            graph_commands::preview_lua,
            graph_commands::check_collapse,
            graph_commands::collapse_to_function,
            graph_commands::search_project,
            graph_commands::find_usages,
            graph_commands::suggest_log_files,
            graph_commands::read_runtime_log,
            graph_commands::parse_runtime_log,
            debugger::debugger_status,
            debugger::debugger_resume,
            test_server::test_server_status,
            test_server::update_test_server_settings,
            test_server::install_test_server,
            test_server::play_test_server,
            test_server::stop_test_server,
            test_server::send_test_server_command,
            session::session_status,
            session::update_session_settings,
            session::host_session,
            session::answer_join_request,
            session::kick_guest,
            session::join_session,
            session::leave_session,
            session::update_presence,
            session::join_session_game,
            mcp::mcp_status,
            mcp::update_mcp_settings,
            mcp::mcp_client_setups,
            mcp::install_mcp_client,
            mcp::report_editor_context,
        ])
        .build(tauri::generate_context!())
        .expect("NanosFlow failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<test_server::TestServerState>().shutdown();
                app.state::<session::SessionState>().shutdown();
                app.state::<mcp::McpState>().shutdown();
            }
        });
}
