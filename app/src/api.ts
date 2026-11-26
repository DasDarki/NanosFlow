import { invoke } from "@tauri-apps/api/core";
import type {
  CatalogPayload,
  Diagnostic,
  FunctionSignature,
  GraphDocument,
  CommittedGraph,
  ConflictChoice,
  FileConflict,
  GraphManifest,
  InterfaceFunction,
  NodeDocument,
  ScopeData,
  VariableDocument,
} from "./graph/model";
import type {
  ApiChannel,
  AppInfo,
  BuildReport,
  CollapseError,
  CollapsedFunction,
  DebuggerStatus,
  LogChunk,
  LogEntry,
  PlayResult,
  ResumeMode,
  TestServerSettings,
  TestServerStatus,
  SessionSettings,
  SessionStatus,
  PresenceUpdate,
  SearchResult,
  Usages,
  EventDirection,
  LuaPreview,
  EntryLocation,
  GraphKind,
  CommandError,
  CreatedProject,
  FieldIssue,
  NewProjectOptions,
  ProjectView,
  RecentProject,
  ScriptContext,
  CustomSetting,
  EditorContext,
  Preferences,
  McpClientKind,
  McpClientSetup,
  McpInstallScope,
  McpSettings,
  McpStatus,
} from "./types";

export const OPEN_PROJECT_EVENT = "open-project-requested";
export const PROJECT_EXTENSION = "nfproj";

export function isCommandError(value: unknown): value is CommandError {
  return typeof value === "object" && value !== null && "title" in value && "message" in value;
}

export function toCommandError(value: unknown): CommandError {
  if (isCommandError(value)) {
    return value;
  }
  return { title: "Something went wrong", message: String(value), issues: [] };
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  defaultProjectOptions: () => invoke<NewProjectOptions>("default_project_options"),
  slugify: (value: string) => invoke<string>("slugify", { value }),
  validateNewProject: (options: NewProjectOptions) =>
    invoke<FieldIssue[]>("validate_new_project", { options }),
  createProject: (options: NewProjectOptions) => invoke<CreatedProject>("create_project", { options }),
  openProject: (path: string) => invoke<ProjectView>("open_project", { path }),
  listRecentProjects: () => invoke<RecentProject[]>("list_recent_projects"),
  removeRecentProject: (path: string) => invoke<void>("remove_recent_project", { path }),
  takeLaunchProject: () => invoke<string | null>("take_launch_project"),
  pathExists: (path: string) => invoke<boolean>("path_exists", { path }),
  nodeCatalog: (channel: ApiChannel) => invoke<CatalogPayload>("node_catalog", { channel }),
  loadGraph: (path: string) => invoke<GraphDocument>("load_graph", { path }),
  saveGraphChanges: (path: string, upserts: NodeDocument[], deletions: string[]) =>
    invoke<void>("save_graph_changes", { path, upserts, deletions }),
  analyzeGraph: (
    channel: ApiChannel,
    packagePath: string,
    graphPath: string,
    context: ScriptContext,
    kind: GraphKind,
    nodes: NodeDocument[],
  ) => invoke<Diagnostic[]>("analyze_graph", { channel, packagePath, graphPath, context, kind, nodes }),
  graphScope: (channel: ApiChannel, packagePath: string, graphPath: string) =>
    invoke<ScopeData>("graph_scope", { channel, packagePath, graphPath }),
  updateFunctionSignature: (channel: ApiChannel, packagePath: string, graphPath: string, signature: FunctionSignature) =>
    invoke<ScopeData>("update_function_signature", { channel, packagePath, graphPath, signature }),
  updateEvent: (
    channel: ApiChannel,
    packagePath: string,
    graphPath: string,
    direction: EventDirection,
    signature: FunctionSignature,
  ) => invoke<GraphManifest>("update_event", { channel, packagePath, graphPath, direction, signature }),
  saveVariables: (channel: ApiChannel, packagePath: string, graphPath: string, variables: VariableDocument[], deletions: string[]) =>
    invoke<ScopeData>("save_variables", { channel, packagePath, graphPath, variables, deletions }),
  updateClass: (
    channel: ApiChannel,
    projectFile: string,
    packagePath: string,
    graphPath: string,
    parent: string,
    description: string,
    interfaces: string[],
    exported: boolean,
  ) =>
    invoke<{ project: ProjectView; scope: ScopeData }>("update_class", {
      channel,
      projectFile,
      packagePath,
      graphPath,
      settings: { parent, description, interfaces, exported },
    }),
  graphConflicts: (projectFile: string, graphPath: string) => invoke<FileConflict[]>("graph_conflicts", { projectFile, graphPath }),
  resolveConflict: (projectFile: string, graphPath: string, file: string, choice: ConflictChoice) =>
    invoke<FileConflict[]>("resolve_conflict", { projectFile, graphPath, file, choice }),
  committedGraph: (projectFile: string, graphPath: string) => invoke<CommittedGraph | null>("committed_graph", { projectFile, graphPath }),
  updateInterface: (channel: ApiChannel, packagePath: string, graphPath: string, description: string, functions: InterfaceFunction[]) =>
    invoke<GraphManifest>("update_interface", { channel, packagePath, graphPath, description, functions }),
  createDispatcher: (projectFile: string, classPath: string, name: string) =>
    invoke<ProjectView>("create_dispatcher", { projectFile, classPath, name }),
  createMethod: (projectFile: string, classPath: string, name: string) =>
    invoke<ProjectView>("create_method", { projectFile, classPath, name }),
  validateEntryName: (name: string) => invoke<FieldIssue[]>("validate_entry_name", { name }),
  createGraph: (channel: ApiChannel, projectFile: string, location: EntryLocation, name: string, kind: GraphKind, parent?: string) =>
    invoke<ProjectView>("create_graph", { channel, projectFile, location, name, kind, parent: parent ?? null }),
  createFolder: (projectFile: string, location: EntryLocation, name: string) =>
    invoke<ProjectView>("create_folder", { projectFile, location, name }),
  renameEntry: (projectFile: string, path: string, name: string) =>
    invoke<ProjectView>("rename_entry", { projectFile, path, name }),
  moveEntry: (projectFile: string, path: string, target: EntryLocation) =>
    invoke<ProjectView>("move_entry", { projectFile, path, target }),
  deleteEntry: (projectFile: string, path: string) => invoke<ProjectView>("delete_entry", { projectFile, path }),
  updatePackageRequirements: (projectFile: string, packagePath: string, requirements: string[]) =>
    invoke<ProjectView>("update_package_requirements", { projectFile, packagePath, requirements }),
  updateOutputDirectory: (projectFile: string, directory: string | null) =>
    invoke<ProjectView>("update_output_directory", { projectFile, directory }),
  buildProject: (channel: ApiChannel, projectFile: string) => invoke<BuildReport>("build_project", { channel, projectFile }),
  previewLua: (channel: ApiChannel, packagePath: string, graphId: string) =>
    invoke<LuaPreview>("preview_lua", { channel, packagePath, graphId }),
  searchProject: (channel: ApiChannel, packagePath: string, query: string) =>
    invoke<SearchResult>("search_project", { channel, packagePath, query }),
  findUsages: (channel: ApiChannel, packagePath: string, target: string) =>
    invoke<Usages>("find_usages", { channel, packagePath, target }),
  suggestLogFiles: (projectFile: string) => invoke<string[]>("suggest_log_files", { projectFile }),
  readRuntimeLog: (projectFile: string, path: string, offset: number | null) =>
    invoke<LogChunk>("read_runtime_log", { projectFile, path, offset }),
  parseRuntimeLog: (projectFile: string, text: string) => invoke<LogEntry[]>("parse_runtime_log", { projectFile, text }),
  debuggerStatus: () => invoke<DebuggerStatus>("debugger_status"),
  debuggerResume: (mode: ResumeMode) => invoke<void>("debugger_resume", { mode }),
  testServerStatus: (projectFile: string) => invoke<TestServerStatus>("test_server_status", { projectFile }),
  updateTestServerSettings: (projectFile: string, settings: TestServerSettings) =>
    invoke<TestServerStatus>("update_test_server_settings", { projectFile, settings }),
  installTestServer: (projectFile: string) => invoke<void>("install_test_server", { projectFile }),
  playTestServer: (channel: ApiChannel, projectFile: string, restart: boolean) =>
    invoke<PlayResult>("play_test_server", { channel, projectFile, restart }),
  stopTestServer: () => invoke<boolean>("stop_test_server"),
  sendTestServerCommand: (line: string) => invoke<void>("send_test_server_command", { line }),
  sessionStatus: () => invoke<SessionStatus>("session_status"),
  updateSessionSettings: (settings: SessionSettings) => invoke<SessionStatus>("update_session_settings", { settings }),
  hostSession: (projectFile: string) => invoke<SessionStatus>("host_session", { projectFile }),
  answerJoinRequest: (guest: number, admit: boolean) => invoke<SessionStatus>("answer_join_request", { guest, admit }),
  kickGuest: (guest: number) => invoke<SessionStatus>("kick_guest", { guest }),
  joinSession: (code: string) => invoke<SessionStatus>("join_session", { code }),
  leaveSession: () => invoke<SessionStatus>("leave_session"),
  updatePresence: (presence: PresenceUpdate) => invoke<void>("update_presence", { presence }),
  joinSessionGame: () => invoke<string>("join_session_game"),
  preferences: () => invoke<Preferences>("preferences"),
  updatePreferences: (preferences: Preferences) => invoke<Preferences>("update_preferences", { preferences }),
  updateCustomSettings: (projectFile: string, packagePath: string, settings: CustomSetting[]) =>
    invoke<ProjectView>("update_custom_settings", { projectFile, packagePath, settings }),
  mcpStatus: () => invoke<McpStatus>("mcp_status"),
  updateMcpSettings: (settings: McpSettings) => invoke<McpStatus>("update_mcp_settings", { settings }),
  mcpClientSetups: (scope: McpInstallScope, projectFile: string | null) => invoke<McpClientSetup[]>("mcp_client_setups", { scope, projectFile }),
  installMcpClient: (kind: McpClientKind, scope: McpInstallScope, projectFile: string | null) =>
    invoke<string>("install_mcp_client", { kind, scope, projectFile }),
  reportEditorContext: (context: EditorContext | null) => invoke<void>("report_editor_context", { context }),
  checkCollapse: (channel: ApiChannel, packagePath: string, graphPath: string, selection: string[]) =>
    invoke<CollapseError | null>("check_collapse", { channel, packagePath, graphPath, selection }),
  collapseToFunction: (channel: ApiChannel, projectFile: string, packagePath: string, graphPath: string, selection: string[], name: string) =>
    invoke<{ project: ProjectView; function: CollapsedFunction }>("collapse_to_function", {
      channel,
      projectFile,
      packagePath,
      graphPath,
      selection,
      name,
    }),
};
