export type ApiChannel = "stable" | "latest";
export type ProjectTemplate = "blank" | "game-mode" | "script" | "map";
export type PackageType = "script" | "game-mode" | "map";
export type ScriptContext = "server" | "client" | "shared";
export type GraphKind = "event-graph" | "function" | "class" | "event" | "struct" | "macro" | "enum" | "dispatcher" | "interface";
export type EventDirection = "to-server" | "to-clients" | "local";
export type IssueSeverity = "error" | "warning";

export interface NewProjectOptions {
  name: string;
  location: string;
  folderName: string;
  template: ProjectTemplate;
  packageId: string;
  packageType: PackageType;
  author: string;
  version: string;
  compatibilityVersion: string;
  contexts: ScriptContext[];
  apiChannel: ApiChannel;
  mapAsset: string | null;
  initializeGit: boolean;
}

export type OptionField = keyof NewProjectOptions | "name";

export interface FieldIssue {
  field: OptionField;
  severity: IssueSeverity;
  message: string;
}

export interface GraphSummary {
  id: string;
  name: string;
  path: string;
  kind: GraphKind;
  context: ScriptContext;
  folders: string[];
  nodeCount: number;
  owner?: string;
  parent?: string;
  exported?: boolean;
}

export interface FolderSummary {
  name: string;
  path: string;
  context: ScriptContext;
  folders: string[];
}

export interface PackageSummary {
  id: string;
  folder: string;
  path: string;
  title: string;
  packageType: PackageType;
  version: string;
  requirements: string[];
  graphs: GraphSummary[];
  folders: FolderSummary[];
}

export interface EntryLocation {
  packagePath: string;
  context: ScriptContext;
  folders: string[];
}

export interface ProjectView {
  id: string;
  name: string;
  projectFile: string;
  root: string;
  apiChannel: ApiChannel;
  packages: PackageSummary[];
  outputDirectory: string;
  customOutputDirectory: string | null;
}

export interface CreatedProject {
  project: ProjectView;
  warnings: string[];
}

export interface RecentProject {
  path: string;
  name: string;
  lastOpened: number;
  exists: boolean;
}

export interface AppInfo {
  version: string;
  apiRevision: string;
  platform: string;
}

export interface CommandError {
  title: string;
  message: string;
  issues: FieldIssue[];
}

export interface GraphReference {
  id: string;
  name: string;
  path: string;
  location: string;
}

export interface BuildProblem {
  severity: "error" | "warning";
  code: string;
  graph?: GraphReference;
  node?: string;
  pin?: string;
  message: string;
  hint?: string;
}

export interface PackageReport {
  folder: string;
  title: string;
  outputDirectory: string;
  written: boolean;
  fileCount: number;
  changedFiles: number;
  removedFiles: number;
  problems: BuildProblem[];
}

export interface BuildReport {
  outputDirectory: string;
  packages: PackageReport[];
  errorCount: number;
  warningCount: number;
}

export type LogSeverity = "error" | "warning" | "info" | "watch";

export interface LogLocation {
  package: string | null;
  file: string;
  line: number;
  graph?: string;
  node?: string;
}

export interface LogEntry {
  severity: LogSeverity;
  text: string;
  locations: LogLocation[];
  watch?: { graph: string; node: string; values: string };
}

export interface LogChunk {
  entries: LogEntry[];
  offset: number;
  restarted: boolean;
}

export interface RuntimeMark {
  errors: number;
  lastError?: string;
  watch?: string;
  paused?: boolean;
}

export interface BreakpointHit {
  id: number;
  location: string;
  graph?: string;
  node?: string;
  side: string;
  values: [string, string][];
  stack: StackFrame[];
  canPause: boolean;
}

export interface StackFrame {
  location: string;
  graph?: string;
  node?: string;
}

export type ResumeMode = "continue" | "into" | "over" | "out";

export type DebuggerMessage = { type: "paused"; hit: BreakpointHit } | { type: "hit"; hit: BreakpointHit } | { type: "resumed" };

export interface DebuggerStatus {
  listening: boolean;
  port: number;
  error: string | null;
  paused: BreakpointHit | null;
}

export type HitKind = "graph" | "node" | "comment" | "literal" | "variable" | "type";

export interface SearchHit {
  kind: HitKind;
  graphId: string;
  graphName: string;
  graphPath: string;
  location: string;
  node?: string;
  title: string;
  detail: string;
}

export interface SearchResult {
  hits: SearchHit[];
  truncated: boolean;
}

export interface Usages {
  subject: string;
  subjectKind: string;
  hits: SearchHit[];
}

export interface CollapseError {
  message: string;
  hint: string;
  nodes: string[];
}

export interface CollapsedFunction {
  path: string;
  id: string;
  call: string;
}

export interface LuaPreview {
  file: string | null;
  code: string;
  problems: BuildProblem[];
}

export type SteamApp = "playtest" | "game";
export type ServerBranch = "public" | "bleeding-edge";

export interface TestServerSettings {
  directory: string | null;
  steamApp: SteamApp;
  branch: ServerBranch;
  port: number;
  map: string | null;
  gameMode: string | null;
  downloadMissing: boolean;
}

export interface LaunchPlan {
  map: string;
  projectMap: string | null;
  gameMode: string | null;
  packages: string[];
  ignoredGameModes: string[];
  loadOrder: string[];
  arguments: string[];
}

export interface TestServerStatus {
  directory: string;
  globalDirectory: string;
  usesGlobal: boolean;
  installed: boolean;
  installing: boolean;
  running: boolean;
  runningHere: boolean;
  settings: TestServerSettings;
  plan: LaunchPlan;
  gameModes: string[];
  address: string;
  logFile: string;
}

export interface InstallProgress {
  phase: string;
  percent: number;
}

export type TestServerMessage =
  | { type: "output"; line: string }
  | { type: "state"; running: boolean; exitCode: number | null }
  | { type: "install"; line: string; progress: InstallProgress | null }
  | { type: "installed"; success: boolean; message: string };

export type PlayAction = "build-failed" | "started" | "reloaded" | "restarted";

export interface PlayResult {
  action: PlayAction;
  report: BuildReport;
  address: string;
}

export interface SessionSettings {
  relay: string;
  name: string;
  guestPort: number;
}

export interface SessionPerson {
  id: number;
  name: string;
}

export type SessionView =
  | { type: "idle" }
  | { type: "starting" }
  | { type: "hosting"; code: string; server: string; projectFile: string; gameRunning: boolean; guests: SessionPerson[]; requests: SessionPerson[] }
  | { type: "waiting"; code: string }
  | { type: "joined"; hostName: string; address: string; projectFile: string; game: { running: boolean; steamApp: string } };

export type SessionTone = "info" | "success" | "warning";

export interface SessionActivity {
  id: number;
  text: string;
  tone: SessionTone;
}

export interface SessionStatus {
  settings: SessionSettings;
  view: SessionView;
  activity: SessionActivity[];
}

export interface SessionNotice {
  title: string;
  message: string;
  tone: SessionTone;
}

export interface SessionMessage {
  status: SessionStatus;
  notice: SessionNotice | null;
}

export interface PresenceUpdate {
  graph: string | null;
  x: number;
  y: number;
  selection: string[];
}

export interface Presence extends PresenceUpdate {
  peer: number;
  name: string;
  gone: boolean;
}

export interface SessionFilesMessage {
  projectRoot: string;
  paths: string[];
  author: string;
}
